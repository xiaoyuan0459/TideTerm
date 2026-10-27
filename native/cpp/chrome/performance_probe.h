#pragma once
// Optional, bounded timing inside the existing isolated memory-probe process.
// Normal QApplication entry points and terminal input are unaffected.
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include "event_cost_probe.h"
#include "window_scenario_probe.h"
#include <QApplication>
#include <QElapsedTimer>
#include <QTimer>
#include <QThread>
#include <QLibrary>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <memory>
#include <vector>

namespace performance_probe {
class Application final : public QApplication {
public:
    Application(int &argc, char **argv) : QApplication(argc, argv),
        enabled_(qEnvironmentVariable("TIDETERM_WINDOW_PERF") == QLatin1String("1")) {
        clock_.start();
        if (enabled_) for (auto &sample : samples_) sample.values.reserve(kLimit);
#ifdef Q_OS_WIN
        if (enabled_) {
            const int parent = qEnvironmentVariableIntValue("TIDETERM_WINDOW_PERF_PARENT_PID");
            if (parent > 0) {
                using LockForeground = int (__stdcall *)(quint32);
                const auto lock = reinterpret_cast<LockForeground>(
                    QLibrary::resolve(QStringLiteral("user32"), "LockSetForegroundWindow"));
                const auto locked = std::make_shared<bool>(false);
                QObject::connect(this, &QGuiApplication::applicationStateChanged, this,
                    [lock, locked](Qt::ApplicationState state) {
                        if (state != Qt::ApplicationActive || *locked) return;
                        *locked = lock && lock(1u); // LSFW_LOCK; user activation still overrides it.
                        std::printf("[window-perf] foreground_lock=%d\n", int(*locked));
                    });
                QObject::connect(this, &QCoreApplication::aboutToQuit, this, [parent, lock, locked] {
                const bool unlocked = *locked && lock && lock(2u); // LSFW_UNLOCK before handoff.
                std::printf("[window-perf] foreground_unlock=%d\n", int(unlocked));
                // Only the foreground probe can grant its own launcher the right
                // to activate the next isolated process. Never grant ASFW_ANY.
                using AllowForeground = int (__stdcall *)(quint32);
                const auto allow = reinterpret_cast<AllowForeground>(
                    QLibrary::resolve(QStringLiteral("user32"), "AllowSetForegroundWindow"));
                const bool granted = allow && allow(quint32(parent));
                std::printf("[window-perf] foreground_handoff=%d\n", int(granted));
                });
            }
        }
#endif
    }

    bool notify(QObject *receiver, QEvent *event) override {
        if (!enabled_ || !armed_ || clock_.elapsed() < 6000) return QApplication::notify(receiver, event);
        if (event_profile_ && QThread::currentThread() == thread()) {
            auto scope = event_costs_.enter(receiver, event);
            event_costs_.activate(scope);
            return timed_notify(receiver, event);
        }
        return timed_notify(receiver, event);
    }

    void arm(QWidget *window) {
        clock_.restart();
        armed_ = true;
        if (enabled_ && window) {
            std::printf("[window-perf] ready_window=%llu\n",
                static_cast<unsigned long long>(window->winId()));
            std::fflush(stdout);
        }
    }

    bool timed_notify(QObject *receiver, QEvent *event) {
        int group = -1;
        if (event->type() == QEvent::UpdateRequest) {
            if (auto *widget = qobject_cast<QWidget *>(receiver); widget && widget->isWindow()) group = 0;
        } else if (event->type() == QEvent::Paint && dynamic_cast<PtyView *>(receiver)) {
            group = 1;
        } else if (event->type() == QEvent::Timer) {
            if (auto *timer = qobject_cast<QTimer *>(receiver);
                timer && (timer->interval() == 20 || timer->interval() == 16) &&
                dynamic_cast<MainWindow *>(timer->parent())) group = 2;
        }
        if (group < 0) return QApplication::notify(receiver, event);
        QElapsedTimer elapsed;
        elapsed.start();
        const bool handled = QApplication::notify(receiver, event);
        const qint64 duration = elapsed.nsecsElapsed();
        samples_[size_t(group)].add(duration);
        if (group == 0) {
            const qint64 now = clock_.nsecsElapsed();
            if (last_frame_ns_) samples_[3].add(now - last_frame_ns_);
            last_frame_ns_ = now;
        }
        return handled;
    }

    void report() const {
        if (!enabled_) return;
        if (event_profile_) event_costs_.report();
        const double seconds = double(clock_.elapsed() - 6000) / 1000.0;
        std::printf("[window-perf] measured_seconds=%.3f window_update_hz=%.3f terminal_paint_hz=%.3f\n",
            seconds, seconds > 0 ? double(samples_[0].count) / seconds : 0,
            seconds > 0 ? double(samples_[1].count) / seconds : 0);
        const char *names[] = {"window-update", "terminal-paint", "main-pump", "frame-gap"};
        for (size_t i = 0; i < samples_.size(); ++i) {
            const auto &sample = samples_[i];
            auto sorted = sample.values;
            std::sort(sorted.begin(), sorted.end());
            auto percentile = [&](double p) {
                if (sorted.empty()) return 0.0;
                const size_t at = size_t(std::ceil(p * double(sorted.size()))) - 1;
                return double(sorted[at]) / 1000.0;
            };
            std::printf("[window-perf] group=%s count=%llu retained=%zu dropped=%llu total_ms=%.3f p50_us=%.3f p95_us=%.3f p99_us=%.3f max_us=%.3f\n",
                names[i], static_cast<unsigned long long>(sample.count), sorted.size(),
                static_cast<unsigned long long>(sample.count - sorted.size()), double(sample.total) / 1e6,
                percentile(0.5), percentile(0.95), percentile(0.99),
                sorted.empty() ? 0.0 : double(sorted.back()) / 1000.0);
        }
    }

private:
    static constexpr size_t kLimit = 4096;
    struct Samples {
        uint64_t count = 0;
        qint64 total = 0;
        std::vector<qint64> values;
        void add(qint64 ns) {
            ++count;
            total += ns;
            if (values.size() < kLimit) values.push_back(ns);
        }
    };
    bool enabled_;
    bool armed_ = false;
    bool event_profile_ = qEnvironmentVariableIsSet("TIDETERM_WINDOW_EVENT_PROFILE");
    EventCosts event_costs_;
    QElapsedTimer clock_;
    qint64 last_frame_ns_ = 0;
    std::array<Samples, 4> samples_;
};

inline void install_load(MainWindow *window, uint8_t stage) {
    if (!window || stage != 4 || qEnvironmentVariable("TIDETERM_WINDOW_PERF") != QLatin1String("1")) return;
    install_scenario(window);
    if (qEnvironmentVariable("TIDETERM_WINDOW_PERF_LOAD") != QLatin1String("mixed")) return;
    // Build synthetic output once after layout. It only enters the output parser;
    // no command, key, clipboard operation or network request is sent.
    QTimer::singleShot(2000, window, [window] {
        auto *view = window->pane_view(0, 0);
        if (!view || view->is_closed()) return;
        const auto stats = term_stats(view->tab());
        QByteArray output;
        for (uint32_t row = 0; row < std::min(stats.rows, 100u); ++row) {
            output += row % 2 ? "\x1b[32m" : "\x1b[0m";
            for (uint32_t col = 0; col + 1 < std::min(stats.cols, 300u); ++col) {
                if (col % 24 == 0 && col + 2 < stats.cols) {
                    output += "\xe4\xb8\xad";
                    ++col;
                } else output += char('A' + (row + col) % 26);
            }
            output += "\x1b[0m\r\n";
        }
        bool interval_ok = false;
        const int requested_interval = qEnvironmentVariableIntValue("TIDETERM_WINDOW_PERF_INTERVAL_MS", &interval_ok);
        const int interval = interval_ok ? qBound(8, requested_interval, 1000) : 50;
        std::printf("[window-perf] load=mixed cols=%u rows=%u bytes=%lld interval_ms=%d\n",
            stats.cols, stats.rows, static_cast<long long>(output.size()), interval);
        auto *load = new QTimer(view);
        load->setTimerType(Qt::PreciseTimer);
        const auto count = std::make_shared<uint64_t>(0);
        QObject::connect(qApp, &QCoreApplication::aboutToQuit, load, [count, bytes = output.size()] {
            std::printf("[window-perf] load_count=%llu payload_bytes=%lld total_bytes=%llu\n",
                static_cast<unsigned long long>(*count), static_cast<long long>(bytes),
                static_cast<unsigned long long>(*count * uint64_t(bytes)));
        });
        QObject::connect(load, &QTimer::timeout, view, [view, output, count] {
            if (view->is_closed()) return;
            term_feed_inject(view->tab(), rust::Slice<const uint8_t>(
                reinterpret_cast<const uint8_t *>(output.constData()), size_t(output.size())));
            ++*count;
        });
        load->start(interval);
    });
}
}

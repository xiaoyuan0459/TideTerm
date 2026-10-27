#pragma once
// Diagnostic-only, fixed storage. Names are Qt classes, never user content.
#include <QElapsedTimer>
#include <QEvent>
#include <QObject>
#include <QTimer>
#include <algorithm>
#include <array>
#include <cstdio>

namespace performance_probe {
class EventCosts {
public:
    struct Scope {
        EventCosts *owner;
        size_t bucket;
        qint64 start;
        qint64 children = 0;
        Scope *parent;
        ~Scope() {
            const qint64 ns = owner->clock_.nsecsElapsed() - start;
            if (parent) parent->children += ns;
            owner->active_ = parent;
            auto &entry = owner->entries_[bucket];
            ++entry.count;
            entry.self += ns - children;
            entry.total += ns;
        }
    };
    EventCosts() { clock_.start(); }
    Scope enter(QObject *receiver, QEvent *event) {
        const char *name = receiver->metaObject()->className();
        const int type = int(event->type());
        auto *timer = qobject_cast<QTimer *>(receiver);
        const int interval = timer ? timer->interval() : -1;
        size_t i = 0;
        for (; i < used_; ++i) {
            const auto &entry = entries_[i];
            if (entry.name == name && entry.type == type && entry.interval == interval) break;
        }
        if (i == used_) {
            if (used_ < entries_.size() - 1) {
                entries_[used_++] = {name, type, interval, 0, 0, 0};
            } else i = entries_.size() - 1;
        }
        return {this, i, clock_.nsecsElapsed(), 0, active_};
    }
    void activate(Scope &scope) { active_ = &scope; }
    void report() const {
        auto ranked = entries_;
        std::sort(ranked.begin(), ranked.end(), [](const Entry &a, const Entry &b) { return a.self > b.self; });
        for (const auto &entry : ranked) if (entry.count) {
            std::printf("[event-cost] class=%s event=%d interval=%d count=%llu self_ms=%.3f inclusive_ms=%.3f\n",
                entry.name, entry.type, entry.interval, static_cast<unsigned long long>(entry.count),
                double(entry.self) / 1e6, double(entry.total) / 1e6);
        }
    }
private:
    struct Entry {
        const char *name = "overflow";
        int type = -1;
        int interval = -1;
        uint64_t count = 0;
        qint64 self = 0;
        qint64 total = 0;
    };
    std::array<Entry, 128> entries_{};
    size_t used_ = 0;
    Scope *active_ = nullptr;
    QElapsedTimer clock_;
};
}

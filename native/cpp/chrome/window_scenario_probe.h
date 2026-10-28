#pragma once
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QTimer>
#include <cstdio>

namespace performance_probe {
// Runs only in the isolated full-window diagnostic, with normal session ownership.
inline void install_scenario(MainWindow *window) {
    if (qEnvironmentVariableIntValue("TIDETERM_WINDOW_PERF_PANES") != 8) return;
    for (int level = 0; level < 3; ++level) {
        QTimer::singleShot(150 + level * 200, window, [window, level] {
            const auto ids = window->pane_ids(0);
            for (size_t i = 0; i < ids.size(); ++i) {
                auto *view = window->pane_view(0, int(i * 2));
                if (!view) continue;
                view->setFocus();
                window->split_active(level == 1 ? Qt::Vertical : Qt::Horizontal);
            }
        });
    }
    QTimer::singleShot(2000, window, [window] {
        const int lines = qBound(0, qEnvironmentVariableIntValue("TIDETERM_WINDOW_PERF_HISTORY_LINES"), 20000);
        QByteArray history;
        for (int row = 0; row < lines; ++row) {
            history += QByteArray::number(row).rightJustified(6, '0');
            history += " synthetic \xe4\xb8\xad\xe6\x96\x87 history\r\n";
        }
        const auto ids = window->pane_ids(0);
        for (const auto id : ids) if (!history.isEmpty()) {
            term_feed_inject(id, rust::Slice<const uint8_t>(
                reinterpret_cast<const uint8_t *>(history.constData()), size_t(history.size())));
        }
        std::printf("[window-scenario] panes=%zu history_lines=%d bytes_per_pane=%lld\n",
            ids.size(), lines, static_cast<long long>(history.size()));
    });
    QTimer::singleShot(5000, window, [window] {
        const auto ids = window->pane_ids(0);
        for (size_t i = 0; i < ids.size(); ++i) {
            auto *view = window->pane_view(0, int(i));
            const auto stats = term_stats(ids[i]);
            std::printf("[window-scenario] pane=%zu width=%d height=%d cols=%u rows=%u\n",
                i, view->width(), view->height(), stats.cols, stats.rows);
        }
        const auto report = term_mem_report();
        std::printf("[window-scenario] phase=loaded %.*s\n", int(report.size()), report.data());
    });
    if (qEnvironmentVariableIntValue("TIDETERM_WINDOW_PERF_RECYCLE") != 1) return;
    QTimer::singleShot(18000, window, [window] {
        for (int i = int(window->pane_ids(0).size()) - 1; i > 0; --i)
            if (auto *view = window->pane_view(0, i)) window->close_pane(view);
        std::printf("[window-scenario] phase=closed panes=%zu\n", window->pane_ids(0).size());
    });
    QTimer::singleShot(22000, window, [] {
        const auto report = term_mem_report();
        std::printf("[window-scenario] phase=settled %.*s\n", int(report.size()), report.data());
    });
}
}

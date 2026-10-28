#pragma once
#include "cpp/bridge_shim.h"
#include <QLabel>
#include <QPointer>
#include <QTimer>
#include <functional>
class PtyView;
namespace chrome {
// One event-driven, nonactivating snapshot. The fetch runs only after a dwell.
class HoverPreview final : public QLabel {
public:
    HoverPreview(QWidget *target, PtyView *source);
    void request(uint64_t key, QPoint global, std::function<QString()> fetch);
    void cancel();
    void refresh();
protected:
    bool eventFilter(QObject *watched, QEvent *event) override;
private:
    HistoryState state() const;
    void display();
    QPointer<QWidget> target_;
    QPointer<PtyView> source_;
    QTimer delay_;
    std::function<QString()> fetch_;
    QPoint anchor_;
    HistoryState shown_{};
    uint64_t key_ = 0;
    uint32_t generation_ = 0;
};
}

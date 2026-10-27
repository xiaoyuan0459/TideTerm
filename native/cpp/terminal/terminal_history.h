#pragma once
#include <QObject>
#include <QPointer>
class PtyView;
class QScrollBar;
namespace chrome {
class HoverPreview;
class TerminalHistory final : public QObject {
public:
    static constexpr int kWidth = 12;
    explicit TerminalHistory(PtyView *view);
    void sync();
    void cancel();
protected:
    bool eventFilter(QObject *watched, QEvent *event) override;
private:
    int value_at(int y) const;
    QPointer<PtyView> view_;
    QScrollBar *bar_;
    HoverPreview *preview_;
};
}

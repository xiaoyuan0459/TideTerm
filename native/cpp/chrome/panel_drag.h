#pragma once
#include <QPoint>
#include <QPointer>
class QDockWidget;
class QMainWindow;
class QWidget;

namespace chrome {
class PanelHeader;

class PanelDrag {
public:
    PanelDrag(QMainWindow *window, QDockWidget *source, PanelHeader *header);
    ~PanelDrag();
    /// 浮窗拖动：`global` 是按下时指针在源面板内的相对位置（跟手用）。
    void set_float_grab(const QPoint &global);
    void update(const QPoint &global);
    void finish(const QPoint &global);
private:
    QPointer<QMainWindow> window_;
    QPointer<QDockWidget> source_;
    QPointer<PanelHeader> header_;
    QPointer<QWidget> preview_;
    QPointer<QWidget> card_;
    QPoint float_grab_;
    bool float_grab_set_ = false;
};
}

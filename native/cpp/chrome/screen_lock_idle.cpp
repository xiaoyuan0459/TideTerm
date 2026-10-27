#include "cpp/chrome/screen_lock.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QEvent>
#include <QTimer>

namespace chrome {
namespace {
class AutoLock final : public QObject {
public:
    explicit AutoLock(MainWindow *window) : QObject(window), window_(window) {
        prefs_lock_activity();
        qApp->installEventFilter(this);
        auto *timer = new QTimer(this);
        timer->setInterval(1000);
        connect(timer, &QTimer::timeout, this, [this] {
            if (window_->property("screenLocked").toBool() || !window_->isVisible()) return;
            if (prefs_lock_idle_due()) lock_screen(window_);
        });
        timer->start();
    }
protected:
    bool eventFilter(QObject *watched, QEvent *event) override {
        if (window_->property("screenLocked").toBool()) return false;
        auto *widget = qobject_cast<QWidget *>(watched);
        auto *owner = widget;
        while (owner && owner != window_) owner = owner->parentWidget();
        if (!owner) return false;
        switch (event->type()) {
        case QEvent::KeyPress:
        case QEvent::InputMethod:
        case QEvent::MouseButtonPress:
        case QEvent::MouseButtonDblClick:
        case QEvent::MouseMove:
        case QEvent::Wheel:
        case QEvent::TouchBegin:
        case QEvent::TouchUpdate:
            prefs_lock_activity();
            break;
        default: break;
        }
        return false;
    }
private:
    MainWindow *window_;
};
}
void install_auto_lock(MainWindow *window) { new AutoLock(window); }
}

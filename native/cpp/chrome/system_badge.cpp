#include "cpp/chrome/system_badge.h"
#include "cpp/chrome/elided_label.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/chrome/app_font.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/i18n.h"
#include <QDateTime>

namespace chrome {
namespace {
class SystemBadge final : public ElidedLabel {
public:
    explicit SystemBadge(PtyView *view) : ElidedLabel(view) {
        setObjectName(QStringLiteral("systemBadge"));
        set_connection_font(this);
        setFocusPolicy(Qt::NoFocus);
        setAlignment(Qt::AlignVCenter | Qt::AlignRight);
        setFixedHeight(20);
        view->installEventFilter(this);
    }
    void place() {
        auto *view = parentWidget();
        const int width = qMax(0, qMin(view->width() - 12, fontMetrics().horizontalAdvance(text()) + 12));
        setGeometry(view->width() - width - 6, view->height() - height() - 4, width, height());
    }
protected:
    bool eventFilter(QObject *, QEvent *event) override {
        if (event->type() == QEvent::Resize) place();
        return false;
    }
};
}
void update_system_badge(PtyView *view) {
    if (!view->isVisible()) return;
    const auto sample = context_system_snapshot(view->tab());
    auto *badge = dynamic_cast<SystemBadge *>(view->findChild<QLabel *>(QStringLiteral("systemBadge")));
    if (!sample.enabled) { if (badge) badge->hide(); return; }
    if (!badge) badge = new SystemBadge(view);
    const QString cpu = sample.cpu_ready ? QString::number(sample.cpu_percent, 'f', 1) + QLatin1Char('%') : QStringLiteral("—");
    const QString memory = sample.ready ? QStringLiteral("%1/%2 GiB")
        .arg(double(sample.used_bytes) / (1024 * 1024 * 1024), 0, 'f', 2)
        .arg(double(sample.total_bytes) / (1024 * 1024 * 1024), 0, 'f', 2) : QStringLiteral("—");
    const QString error = QString::fromUtf8(sample.error.data(), int(sample.error.size()));
    badge->setText(error.isEmpty()
        ? QStringLiteral("T %1   C %2   M %3").arg(QTime::currentTime().toString(QStringLiteral("HH:mm")), cpu, memory)
        : QStringLiteral("Sysmon: %1").arg(error));
    badge->setToolTip(error.isEmpty() ? i18n::tr("系统 CPU 和内存用量，每 5 秒采样") : error);
    badge->place();
    badge->show();
    badge->raise();
}
}

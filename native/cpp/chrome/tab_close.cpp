// tab_close 实现（对齐批次 10）。QMessageBox 的默认按钮 = 「关闭」⇒ Enter 确认；
// EscapeRole ⇒ Esc 取消；Qt 的模态框自带「关闭后焦点归还原控件」，参考
// close-confirm.ts:15,41 的 prevFocus?.focus() 由框架承担（记录为环境适配，非缺陷）。
#include "cpp/chrome/tab_close.h"
#include "cpp/i18n.h"

#include "cpp/main_window.h"
#include "cpp/chrome/ui_theme.h"

#include <QApplication>
#include <QMessageBox>
#include <QMouseEvent>
#include <QTabBar>
#include <QCheckBox>
#include <QPushButton>
#include <QSettings>
#include <QDebug>

namespace {

/// 中键按下 = 关闭该标签（参考 `TabBar/index.ts:181-187` 的 auxclick button===1）。
class MiddleClickFilter : public QObject {
public:
    explicit MiddleClickFilter(std::function<void(int)> cb) : on_close_(std::move(cb)) {}

protected:
    bool eventFilter(QObject *watched, QEvent *ev) override {
        auto *me = dynamic_cast<QMouseEvent *>(ev);
        auto *bar = dynamic_cast<QTabBar *>(watched);
        if (me != nullptr && bar != nullptr && me->type() == QEvent::MouseButtonRelease &&
            me->button() == Qt::MiddleButton) {
            const int at = bar->tabAt(me->pos());
            if (at >= 0 && on_close_) {
                on_close_(at);
                return true; // 已消费，避免透传成选中
            }
        }
        return false;
    }

private:
    std::function<void(int)> on_close_;
};

} // namespace

namespace chrome {

bool window_close_confirmation_enabled() {
    QSettings settings(QSettings::defaultFormat(), QSettings::UserScope,
        QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
    return settings.value(QStringLiteral("ui/confirmWindowClose.v1"), true).toBool();
}

bool set_window_close_confirmation(bool enabled) {
    QSettings settings(QSettings::defaultFormat(), QSettings::UserScope,
        QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
    settings.setValue(QStringLiteral("ui/confirmWindowClose.v1"), enabled);
    settings.sync();
    return settings.status() == QSettings::NoError;
}

bool confirm_close_window(QWidget *parent, int sessions) {
    if (!window_close_confirmation_enabled()) return true;
    QMessageBox box(QMessageBox::Warning, i18n::tr("关闭"), i18n::tr("是否要关闭此窗口？"),
                    QMessageBox::Yes | QMessageBox::No, parent);
    box.setObjectName(QStringLiteral("windowCloseConfirm"));
    box.setInformativeText(i18n::tr("将关闭该窗口中的 %1 个终端会话。").arg(sessions));
    box.button(QMessageBox::Yes)->setText(i18n::tr("是(&Y)"));
    box.button(QMessageBox::No)->setText(i18n::tr("否(&N)"));
    box.setDefaultButton(QMessageBox::No);
    box.setEscapeButton(QMessageBox::No);
    auto *remember = new QCheckBox(i18n::tr("不再显示此消息"), &box);
    box.setCheckBox(remember);
    box.button(QMessageBox::No)->setStyleSheet(QStringLiteral("border: 1px solid %1;")
        .arg(chrome::ui_colors().accent.name()));
    box.exec();
    if (box.clickedButton() != box.button(QMessageBox::Yes)) return false;
    if (remember->isChecked() && !set_window_close_confirmation(false))
        qWarning("Could not save window close confirmation preference");
    return true;
}

bool confirm_close_tab(QWidget *parent, const QString &tab_label) {
    QMessageBox box(parent);
    box.setIcon(QMessageBox::Question);
    box.setWindowTitle(i18n::tr("确认关闭"));
    box.setText(QStringLiteral("%1").arg(tab_label));
    box.setInformativeText(i18n::tr("该终端有进程正在运行，确定关闭吗？"));
    auto *ok = box.addButton(i18n::tr("关闭"), QMessageBox::AcceptRole);
    box.addButton(i18n::tr("取消"), QMessageBox::RejectRole);
    box.setDefaultButton(ok); // Enter = 确认（参考 onKeydown Enter → finish(true)）
    return box.exec() == QMessageBox::Accepted;
}

void install_middle_click_close(QTabBar *bar, std::function<void(int)> on_close) {
    if (bar == nullptr || !on_close) {
        return;
    }
    bar->installEventFilter(new MiddleClickFilter(std::move(on_close)));
}

void request_close_tab(MainWindow *win, int index) {
    if (win == nullptr || index < 0 || index >= win->page_count()) {
        return;
    }
    const QString label = QString::fromStdString(win->tab_title(index));
    // 策略可注入：自检替换；未来按「真有进程在跑」判定（参考 app.ts:811-814 的 TODO）。
    const bool confirmed = win->close_confirm_ask
                               ? win->close_confirm_ask(label)
                               : confirm_close_tab(win, label);
    if (confirmed) {
        win->close_tab(index);
    }
}

} // namespace chrome

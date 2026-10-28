// User-selected shortcuts to existing menu actions. Inline strip for the session header row
// (2026-10-01: merged into the connection-bar row — WindTerm pane header style; the previous
// window-wide QToolBar row above the tab bar was visually disconnected from the address row).
//
// 2026-10-01 WindTerm 页头实拍取证对齐：默认四件套 = □⁺新建会话(带协议下拉) / □✕断开 /
// □○重连 / ∞快速连接，右端 ⠸▾ 是自定义入口。动作集仍可从
// 自定义菜单增删（持久化），协议下拉与标签栏 + 共用 fill_new_session_menu。
#include "cpp/chrome/favorite_toolbar.h"

#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/tab_quick_bar.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"

#include <QAction>
#include <QHBoxLayout>
#include <QLayoutItem>
#include <QMenu>
#include <QMenuBar>
#include <QSettings>
#include <QTimer>
#include <QToolButton>
#include <cstring>

namespace chrome {
namespace {

struct Choice {
    const char *id;
    const char *label;
    const char *icon;
};

constexpr Choice kChoices[] = {
    // 图标语义对齐 WT 页头字形（□⁺ 新建 / □✕ 断开；□○ 留给复制窗格类备选）：
    // 分割/缩放等窗格操作 WT 放在 ⋮ 菜单，这里作为备选可勾选。
    {"session:new", "新建会话", "pane_add"},
    {"session:disconnect", "断开会话", "pane_close"},
    {"session:reconnect", "重新连接", "pane_reconnect"},
    {"panel:quick", "快速连接", "link"},
    {"window:split-view-horizontal", "水平分割窗格", "split_h"},
    {"window:split-view-vertical", "垂直分割窗格", "split_v"},
    {"window:close-pane", "关闭窗格", "close"},
    {"window:zoom-pane", "放大窗格", "zoom"},
    {"session:open", "打开会话", "open"},
    {"search:find", "查找", "search"},
    {"panel:sftp", "文件管理器", "folder"},
};

constexpr const char *kSettingsKey = "chrome/favoriteActions.v1";

/// 页头左簇：紧凑图标条（无 QToolBar 外壳——不再需要独立底色与行高）。
class FavoriteActions final : public QWidget {
public:
    explicit FavoriteActions(MainWindow *window) : QWidget(window), window_(window) {
        setObjectName(QStringLiteral("favoriteActions"));
        lay_ = new QHBoxLayout(this);
        lay_->setContentsMargins(0, 0, 0, 0);
        lay_->setSpacing(1);
        QSettings settings(QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
        const QStringList fallback = {QStringLiteral("session:new"),
                                      QStringLiteral("session:disconnect"),
                                      QStringLiteral("session:reconnect")};
        chosen_ = settings.value(QLatin1String(kSettingsKey), fallback).toStringList();
        // v1 旧默认（分割×2/关格/重连/快速连接）与 v2（四件套，panel:quick 与页头
        // 「>」同动作属重复——2026-10-02 用户反馈）都升级到 v3 三件套；用户自定义
        // 过且非旧默认的集合不动。
        const QStringList legacy = {QStringLiteral("window:split-view-horizontal"),
                                    QStringLiteral("window:split-view-vertical"),
                                    QStringLiteral("window:close-pane"),
                                    QStringLiteral("session:reconnect"),
                                    QStringLiteral("panel:quick")};
        const QStringList v2 = {QStringLiteral("session:new"),
                                QStringLiteral("session:disconnect"),
                                QStringLiteral("session:reconnect"),
                                QStringLiteral("panel:quick")};
        if (chosen_ == legacy || chosen_ == v2) chosen_ = fallback;
        rebuild();
    }

private:
    QToolButton *make_button(const QString &icon, const QString &tip, int width = -1) {
        auto *button = new QToolButton(this);
        button->setObjectName(QStringLiteral("favoriteAction"));
        set_button_icon(button, icon, metrics::kSessionHeaderGlyphSize, IconInk::Primary);
        // Session actions share one hit area; the protocol menu uses the same button.
        button->setFixedSize(width > 0 ? width : metrics::kHeaderToolButtonSize,
                             metrics::kHeaderToolButtonSize);
        button->setToolTip(tip);
        button->setAccessibleName(tip);
        return button;
    }

    /// 新建动作落组前置：图标条住在 SessionBanner 里（其父即页 root）。
    /// SessionBanner 无 Q_OBJECT（ADR-014）⇒ dynamic_cast。
    void activate_own_group() {
        if (auto *banner = dynamic_cast<SessionBanner *>(parentWidget())) {
            if (auto *group = window_->workspace()->owner(banner->parentWidget())) {
                window_->activate_group(group);
            }
        }
    }

    void rebuild() {
        while (QLayoutItem *item = lay_->takeAt(0)) {
            delete item->widget();
            delete item;
        }
        for (const Choice &choice : kChoices) {
            if (!chosen_.contains(QLatin1String(choice.id))) continue;
            QAction *source = find_menu_action(window_->menuBar(), QLatin1String(choice.id));
            if (!source) continue;
            QToolButton *button = make_button(QLatin1String(choice.icon), i18n::tr(choice.label),
                                              -1);
            button->setAutoRaise(true);
            button->setFocusPolicy(Qt::NoFocus);
            button->setEnabled(source->isEnabled());
            connect(source, &QAction::changed, button,
                    [button, source] { button->setEnabled(source->isEnabled()); });
            connect(button, &QToolButton::clicked, source, &QAction::trigger);
            if (std::strcmp(choice.id, "session:new") == 0) {
                // WT 页头 □⁺▾（2026-10-01 实拍）：按钮整体弹协议菜单。首项=新建会话
                // 表单，其余为本机终端/SSH/Telnet/Serial/TCP 快捷项（与标签栏 + 共源）。
                // 不用 MenuButtonPopup：22px 高的按钮箭头触控区只有 ~15px，实机难命中。
                auto *proto = new QMenu(button);
                style_menu(proto);
                proxy_action(proto, source, QStringLiteral("pane_add"), i18n::tr("新建会话表单…"));
                proto->addSeparator();
                fill_new_session_menu(proto, window_, [this] { activate_own_group(); });
                button->setMenu(proto);
                button->setPopupMode(QToolButton::InstantPopup);
            }
            lay_->addWidget(button);
        }
        QToolButton *customize = make_button(QStringLiteral("hdr_menu"), i18n::tr("自定义工具栏"));
        customize->setObjectName(QStringLiteral("favoriteCustomize"));
        auto *menu = new QMenu(customize);
        style_menu(menu);
        for (const Choice &choice : kChoices) {
            QAction *source = find_menu_action(window_->menuBar(), QLatin1String(choice.id));
            if (!source) continue;
            QAction *item = menu->addAction(i18n::tr(choice.label));
            item->setCheckable(true);
            item->setChecked(chosen_.contains(QLatin1String(choice.id)));
            connect(item, &QAction::toggled, this, [this, id = QString::fromLatin1(choice.id)](bool on) {
                if (on && !chosen_.contains(id)) chosen_.append(id);
                else if (!on) chosen_.removeAll(id);
                QSettings settings(QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
                settings.setValue(QLatin1String(kSettingsKey), chosen_);
                QTimer::singleShot(0, this, [this] { rebuild(); });
            });
        }
        customize->setMenu(menu);
        customize->setPopupMode(QToolButton::InstantPopup);
        lay_->addWidget(customize);
    }

    MainWindow *window_;
    QHBoxLayout *lay_;
    QStringList chosen_;
};

} // namespace

QWidget *build_favorite_actions(MainWindow *window) {
    return new FavoriteActions(window);
}

} // namespace chrome

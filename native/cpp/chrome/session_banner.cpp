// SessionBanner 实现：横幅控件编排 + Rust 快照刷新。
//
// 2026-10-01 重做：旧版是 [ⓘ][协议 chip][只读 QLineEdit] … [几何文字][⋮]，被用户指出
// 丑、字体差、图标糊；新版按 WindTerm 地址栏实拍对齐为单行页头。
// 2026-10-02 面包屑分段化：地址从只读 QLabel 改为 `协议 ▸ 分组 ▸ 会话名` 分段按钮——
// WT 实拍取证（wt_crumb_click2.png）：面包屑每段可点击，会话段点击弹同级会话列表。
// 协议段=新建协议菜单（fill_new_session_menu）；分组段=组内会话；会话段=同级会话。

#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/app_font.h"

#include "cpp/bridge_shim.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/display_names.h"
#include "cpp/chrome/icon_label.h"
#include "cpp/chrome/menu_actions.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/quick_connect.h"
#include "cpp/chrome/session_vault.h"
#include "cpp/chrome/tab_quick_bar.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"

#include <QEvent>
#include <QFrame>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QKeyEvent>
#include <QMenu>
#include <QMessageBox>
#include <QToolButton>
#include <QStackedWidget>
#include <QStandardPaths>
#include <QTimer>

namespace {

QString qstr(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

/// 协议名（与 ⓘ 信息卡同表：0=本地 1=SSH 2=Telnet 3=串口 4=TCP）。
QString proto_name(int kind) {
    return chrome::protocol_display_name(kind);
}

/// 会话树行 → 展开分组栈，收集 `group_path` 分组下的会话（id + 展示名）。
/// SessRow 是缩进平铺（kind 0=分组 1=会话），按 depth 维护分组栈重建路径。
void collect_group_sessions(const rust::Vec<SessRow> &rows, const QString &target,
                            QVector<QPair<QString, QString>> *out) {
    QStringList stack;
    for (const SessRow &r : rows) {
        if (r.kind == 0) {
            stack.resize(int(r.depth));
            stack.push_back(qstr(r.label));
        } else if (stack.join(QLatin1Char('>')) == target) {
            out->append({qstr(r.id), qstr(r.label)});
        }
    }
}

} // namespace

QToolButton *SessionBanner::make_icon_button(const QString &name, const QString &tip) {
    auto *b = new QToolButton(this);
    const int g = chrome::metrics::kSessionHeaderGlyphSize;
    // 页头图标取主文字色：WindTerm 参考里该行图标与地址同为亮色（实拍对照）。
    chrome::set_button_icon(b, name, g, chrome::IconInk::Primary);
    b->setIconSize(QSize(g, g));
    b->setToolTip(tip);
    b->setAccessibleName(tip);
    b->setAutoRaise(true);
    b->setFocusPolicy(Qt::NoFocus);
    b->setFixedSize(chrome::metrics::kSessionHeaderIconSize,
                    chrome::metrics::kSessionHeaderIconSize);
    return b;
}

void SessionBanner::set_lead_widget(QWidget *lead) {
    if (lead == nullptr || lead_ != nullptr) return;
    lead_ = lead;
    lead_->setParent(this);
    // 与 WindTerm 参考一致：动作图标条与 ⓘ 同属左簇、之间只留间距，不加竖线；
    // 竖线留在 ⓘ 与 ▶ 之间。
    auto *lay = static_cast<QHBoxLayout *>(layout());
    lay->insertWidget(0, lead_);
    lay->insertSpacing(1, 2);
}

SessionBanner::SessionBanner(QWidget *parent) : QWidget(parent) {
    // WT 单行页头：[动作图标条][ⓘ] │ [▶] 面包屑 ……… [>] [⌄] [⋮]。
    setObjectName(QStringLiteral("sessionBanner"));
    setAttribute(Qt::WA_StyledBackground, true);
    setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    setFixedHeight(chrome::metrics::kSessionHeaderHeight);
    auto *lay = new QHBoxLayout(this);
    lay->setContentsMargins(4, 2, 6, 2);
    lay->setSpacing(1);
    auto *target = new QWidget(this);
    target->setObjectName(QStringLiteral("sessionTarget"));
    target->setAttribute(Qt::WA_StyledBackground, true);
    target->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    auto *target_row = new QHBoxLayout(target);
    target_row->setContentsMargins(2, 0, 2, 0);
    target_row->setSpacing(1);
    lay->addWidget(target, 1);

    // ── 左簇：ⓘ | ▶ ──
    info_btn_ = make_icon_button(QStringLiteral("info"), i18n::tr("会话信息"));
    connect(info_btn_, &QToolButton::clicked, this, [this] {
        const SessCfg cfg = sess_active_config(session_);
        QString detail = i18n::tr("名称：%1\n协议：%2\n分组：%3")
            .arg(MainWindow::q_title(session_), proto_name(int(cfg.kind)), qstr(cfg.group_path));
        if (cfg.kind == 1) detail += i18n::tr("\n主机：%1:%2\n用户：%3")
            .arg(qstr(cfg.ssh_host)).arg(cfg.ssh_port).arg(qstr(cfg.ssh_user));
        if (cfg.kind == 2) detail += i18n::tr("\n主机：%1:%2")
            .arg(qstr(cfg.telnet_host)).arg(cfg.telnet_port);
        if (cfg.kind == 3) detail += i18n::tr("\n串口：%1")
            .arg(qstr(cfg.serial_path));
        if (cfg.kind == 4) detail += i18n::tr("\n主机：%1:%2")
            .arg(qstr(cfg.tcp_host)).arg(cfg.tcp_port);
        QMessageBox::information(this, i18n::tr("会话信息"), detail);
    });
    target_row->addWidget(info_btn_);

    // 1px 竖分隔（WT 参考里 ⓘ 与 ▶ 之间的细线）。
    auto *sep = new QWidget(this);
    sep->setObjectName(QStringLiteral("headerSep"));
    sep->setFixedSize(1, chrome::metrics::kSessionHeaderGlyphSize);
    target_row->addSpacing(1);
    target_row->addWidget(sep);
    target_row->addSpacing(1);

    // ▶：发送——展开并聚焦本页发送输入（真实能力，不放假钮）。
    send_btn_ = make_icon_button(QStringLiteral("play"), i18n::tr("发送命令行"));
    send_btn_->setIconSize(QSize(8, 8));
    send_btn_->setFixedWidth(16);
    target_row->addWidget(send_btn_);

    // ── 面包屑：`协议 ▸ 分组 ▸ 会话名` 分段按钮（rebuild_crumb 按 bind 重建）──
    crumb_ = new QWidget(this);
    crumb_->setObjectName(QStringLiteral("sessionCrumb"));
    chrome::set_ui_text_font(crumb_);
    crumb_->installEventFilter(this);
    auto *crumb_lay = new QHBoxLayout(crumb_);
    crumb_lay->setContentsMargins(0, 0, 0, 0);
    crumb_lay->setSpacing(2);
    crumb_->setMinimumWidth(40);
    crumb_->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    address_stack_ = new QStackedWidget(target);
    address_stack_->setObjectName(QStringLiteral("sessionAddressStack"));
    address_stack_->setFrameShape(QFrame::NoFrame);
    address_stack_->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    address_stack_->addWidget(crumb_);
    address_ = new QLineEdit(target);
    address_->setObjectName(QStringLiteral("sessionAddress"));
    address_->setFrame(false);
    chrome::set_ui_text_font(address_);
    address_->setAccessibleName(i18n::tr("快速连接"));
    address_->setPlaceholderText(i18n::tr("快速连接：user@host 或 ssh://host"));
    address_->installEventFilter(this);
    address_stack_->addWidget(address_);
    connect(address_, &QLineEdit::returnPressed, this, [this] { connect_address(); });
    target_row->addWidget(address_stack_, 1);

    // 几何信息不进版式：仍按 Rust POD 更新，供自检（grid_text）与 tooltip 使用。
    geom_ = new QLabel(this);
    geom_->setObjectName(QStringLiteral("mutedText"));
    geom_->hide();

    // ── 右簇：> ⌄ ⋮ ──
    go_btn_ = make_icon_button(QStringLiteral("chevron_right"), i18n::tr("转到（快速连接）…"));
    recent_btn_ = make_icon_button(QStringLiteral("down"), i18n::tr("打开会话 / 最近打开"));
    dots_btn_ = make_icon_button(QStringLiteral("more"), i18n::tr("窗格菜单"));
    target_row->addWidget(go_btn_);
    target_row->addWidget(recent_btn_);
    lay->addWidget(dots_btn_);

}

void SessionBanner::rebuild_crumb() {
    if (crumb_ == nullptr) return;
    auto *crumb_lay = static_cast<QHBoxLayout *>(crumb_->layout());

    // 目标分段（bind 每 ~200ms 走一次：分段没变就不重建，避免闪烁/焦点丢失）。
    const SessCfg cfg = sess_active_config(session_);
    QStringList want;
    const QStringList groups =
        qstr(cfg.group_path).split(QLatin1Char('>'), Qt::SkipEmptyParts);
    if (cfg.kind == 0 || !groups.isEmpty()) want << proto_name(int(cfg.kind));
    if (cfg.kind == 0 && groups.isEmpty()) want << i18n::tr("Shell 会话");
    for (const QString &g : groups) want << g.trimmed();
    QString name = qstr(term_title(session_));
    if (auto *win = dynamic_cast<MainWindow *>(window())) name = win->q_title(session_);
    if (!name.isEmpty()) want << name;

    QStringList have;
    for (int i = 0; i < crumb_lay->count(); ++i) {
        if (auto *b = qobject_cast<QToolButton *>(crumb_lay->itemAt(i)->widget()))
            have << b->text();
    }
    if (have == want) return;

    while (QLayoutItem *item = crumb_lay->takeAt(0)) {
        delete item->widget();
        delete item;
    }
    auto *win = dynamic_cast<MainWindow *>(window());

    // 逐段装配：协议段=新建协议菜单；分组段=该分组会话列表；会话段=同级会话列表
    // （WT 面包屑实拍行为；当前会话置勾）。win 为空（offscreen 自检）时只摆文字。
    QString prefix; // 含尾 '>' 的已走分组路径
    for (int i = 0; i < want.size(); ++i) {
        if (i > 0) {
            auto *arrow = new chrome::IconLabel(crumb_);
            arrow->setObjectName(QStringLiteral("crumbSep"));
            arrow->set_glyph(QStringLiteral("chevron_right"), 7);
            arrow->setFixedWidth(9);
            crumb_lay->addWidget(arrow);
        }
        const bool is_proto = i == 0 && want.size() > 1;
        const bool is_session = i == want.size() - 1;
        auto *seg = new QToolButton(crumb_);
        seg->setObjectName(QStringLiteral("crumbSeg"));
        seg->setText(want[i]);
        seg->setAutoRaise(true);
        seg->setFocusPolicy(Qt::NoFocus);
        seg->setToolTip(address_text_ + QStringLiteral("\n") + geom_->text());
        seg->setAccessibleName(want[i]);
        seg->installEventFilter(this);
        if (is_proto && win != nullptr) {
            // 协议段：新建会话协议菜单（与页头 □⁺、标签栏 + 同源）。
            auto *proto = new QMenu(seg);
            chrome::style_menu(proto);
            chrome::fill_new_session_menu(proto, win, [this, win] {
                if (auto *g = win->workspace()->owner(parentWidget())) win->activate_group(g);
            });
            seg->setMenu(proto);
            seg->setPopupMode(QToolButton::InstantPopup);
        } else if (win != nullptr) {
            // 分组段：该分组下的会话；会话段：同级（所在分组）会话。
            const QString base = prefix.endsWith(QLatin1Char('>')) ? prefix.chopped(1) : prefix;
            const QString target =
                is_session ? base
                           : (base.isEmpty() ? want[i] : base + QStringLiteral(">") + want[i]);
            connect(seg, &QToolButton::clicked, seg, [this, seg, target] {
                auto *win2 = dynamic_cast<MainWindow *>(window());
                auto *menu = new QMenu(seg);
                chrome::style_menu(menu);
                QVector<QPair<QString, QString>> sessions;
                collect_group_sessions(sess_tree_rows(), target, &sessions);
                const bool local_profiles = sess_active_config(session_).kind == 0 &&
                    qstr(sess_active_config(session_).group_path).isEmpty();
                if (local_profiles) {
                    for (const QString &shell : {QStringLiteral("powershell"), QStringLiteral("cmd"),
                         QStringLiteral("bash"), QStringLiteral("zsh"), QStringLiteral("fish")}) {
                        if (QStandardPaths::findExecutable(shell).isEmpty()) continue;
                        chrome::add_action(menu, QStringLiteral("terminal"), chrome::shell_display_name(shell), true, [this, win2, shell] {
                            win2->activate_group(win2->workspace()->owner(parentWidget()));
                            const uint32_t id = context_open_shell(rust::String(shell.toStdString()));
                            if (id != UINT32_MAX) win2->add_tab_with(id);
                        });
                    }
                    if (!sessions.isEmpty()) menu->addSeparator();
                }
                if (sessions.isEmpty() && !local_profiles) {
                    menu->addAction(i18n::tr("（该分组没有已保存会话）"))->setEnabled(false);
                }
                const QString current_id = qstr(sess_active_config(session_).id);
                for (const auto &s : sessions) {
                    QAction *a = chrome::add_action(
                        menu, QStringLiteral("terminal"), s.second, true,
                        [this, win2, id = s.first] {
                            const uint32_t tab = chrome::sess_open_with_vault(
                                win2, rust::String(id.toUtf8().toStdString()));
                            if (tab != UINT32_MAX && tab != chrome::kVaultLocked) {
                                win2->activate_group(win2->workspace()->owner(parentWidget()));
                                win2->add_tab_with(tab);
                                chrome::record_recent(id);
                            }
                            win2->refresh_chrome();
                        });
                    a->setCheckable(true);
                    a->setChecked(s.first == current_id);
                }
                menu->exec(seg->mapToGlobal(QPoint(0, seg->height())));
                menu->deleteLater();
            });
        }
        crumb_lay->addWidget(seg);
        if (!is_proto && !is_session) prefix += want[i] + QStringLiteral(">");
    }
    crumb_lay->addStretch(1); // 段靠左（WT 面包屑紧跟 ▶），多余空间留在尾部
}

void SessionBanner::edit_address() {
    if (connecting_) return;
    if (auto *win = dynamic_cast<MainWindow *>(window()))
        win->activate_group(win->workspace()->owner(parentWidget()));
    address_->setText(address_text_);
    address_stack_->setCurrentWidget(address_);
    address_->setFocus();
    address_->selectAll();
}

void SessionBanner::connect_address() {
    if (connecting_) return;
    const QString value = address_->text().trimmed();
    if (value.isEmpty()) { edit_address(); return; }
    auto *win = dynamic_cast<MainWindow *>(window());
    if (!win) return;
    win->activate_group(win->workspace()->owner(parentWidget()));
    connecting_ = true;
    address_->setReadOnly(true);
    const uint32_t id = run_quick_connect(win, value);
    connecting_ = false;
    address_->setReadOnly(false);
    address_stack_->setCurrentWidget(crumb_);
    if (id != UINT32_MAX) {
        win->add_tab_with(id);
    } else if (auto *view = win->active_view()) view->setFocus();
}

void SessionBanner::resizeEvent(QResizeEvent *e) {
    // Keep the address usable in narrow split groups; hidden operations remain in menus.
    const int w = width();
    info_btn_->setVisible(w >= 300);
    send_btn_->setVisible(w >= 300);
    go_btn_->setVisible(w >= 220);
    recent_btn_->setVisible(w >= 220);
    if (lead_ != nullptr) {
        lead_->setVisible(w >= 420);   // 窄窗格先让位给地址，动作仍在菜单/右键里
    }
    QWidget::resizeEvent(e);
}

void SessionBanner::bind(uint32_t session_id) {
    if (session_ != session_id) address_stack_->setCurrentWidget(crumb_);
    session_ = session_id;
    address_text_ = qstr(term_session_url(session_id));
    // 几何走 POD 通道（term_stats.cols/rows），不从 term_status 显示串里 scrape
    // ——显示串是给人看的，格式一改这里就静默显错。
    const TermStats st = term_stats(session_id);
    geom_->setText(QStringLiteral("%1×%2 · UTF-8").arg(st.cols).arg(st.rows));
    rebuild_crumb();
}

QString SessionBanner::breadcrumb() const {
    return address_text_;
}

QString SessionBanner::grid_text() const {
    return geom_->text();
}

void SessionBanner::set_actions(Action pane_menu, Action send_action, Action recent_action) {
    for (auto *button : findChildren<QToolButton *>()) button->installEventFilter(this);
    if (pane_menu) {
        connect(dots_btn_, &QToolButton::clicked, this, [pane_menu] { pane_menu(); });
    }
    if (send_action) {
        connect(send_btn_, &QToolButton::clicked, this, [send_action] { send_action(); });
    } else {
        send_btn_->setEnabled(false);
    }
    connect(go_btn_, &QToolButton::clicked, this, [this] {
        if (address_stack_->currentWidget() == address_) connect_address();
        else edit_address();
    });
    if (recent_action) {
        connect(recent_btn_, &QToolButton::clicked, this, [recent_action] { recent_action(); });
    } else {
        recent_btn_->setEnabled(false);
    }
}

bool SessionBanner::eventFilter(QObject *watched, QEvent *event) {
    if (watched == address_ && event->type() == QEvent::KeyPress &&
        static_cast<QKeyEvent *>(event)->key() == Qt::Key_Escape) {
        address_stack_->setCurrentWidget(crumb_);
        if (auto *win = dynamic_cast<MainWindow *>(window()))
            if (auto *view = win->active_view()) view->setFocus();
        return true;
    }
    if (watched == address_ && event->type() == QEvent::FocusOut) {
        QTimer::singleShot(0, this, [this] {
            if (!connecting_ && !address_->hasFocus()) address_stack_->setCurrentWidget(crumb_);
        });
    }
    if (watched == crumb_ && event->type() == QEvent::MouseButtonPress) {
        edit_address();
        return true;
    }
    if (event->type() == QEvent::MouseButtonPress) {
        if (auto *win = dynamic_cast<MainWindow *>(window()))
            win->activate_group(win->workspace()->owner(parentWidget()));
    }
    return QWidget::eventFilter(watched, event);
}

#include "cpp/chrome/selftest_settings.h"
// --c9-selftest：webview 度量对齐断言（对齐批次 A/B）。
//
// 既有 chrome 自检只断言 objectName / 精确文案 / 终端内容，**从不校验几何与配色**
// （见 docs/reports/qt-webview-ui-parity-2026-09-18.md §7），故布局回归此前不可见。
// 本套件把参考度量钉成断言：改一个高度/宽度/取色就会红。
#include <QApplication>
#include <QDockWidget>
#include <QLabel>
#include <QMenuBar>
#include <QSplitter>
#include <QStatusBar>
#include <QStyledItemDelegate>
#include <QTabBar>
#include <QDir>
#include <QFile>
#include <QMouseEvent>
#include <QMimeData>
#include <QToolButton>
#include <QTreeView>
#include <cstdio>
#include <functional>
#include "cpp/i18n.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/pod_tree_model.h"
#include "cpp/chrome/panel_header.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/split_ratio.h"
#include "cpp/chrome/tree_menu.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/chrome/selftest_chrome_layout.h"
#include "cpp/chrome/selftest_screen_lock.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/chrome/selftest_history.h"
namespace {
rust::String rs(const QString &v) {
    return rust::String(v.toUtf8().toStdString());
}
void check(bool ok, const char *what, const QString &detail = {}) {
    std::printf("[cpp] C9 %s: %s%s\n", what, ok ? "通过" : "失败",
                ok || detail.isEmpty() ? "" : qPrintable(QStringLiteral("（%1）").arg(detail)));
}
/// 断言标签带度量值时用（标签里嵌 kXxx 常量，避免改令牌后文案漂移）。
void check(bool ok, const QString &what, const QString &detail = {}) {
    check(ok, what.toUtf8().constData(), detail);
}
} // namespace
/// 度量对齐自检：五区高度 / dock 钳位 / QSS 令牌覆盖 / 自绘取色 / 树行度量。
int qt_boot_c9(rust::Str png_out) {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    static int argc = 1;
    static char arg0[] = "native-c9";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();
    namespace m = chrome::metrics;
    bool ok = true;
    MainWindow w;
    w.show();
    ok &= check_chrome_layout(w);

    // 1) 五区固定高度（WT 基准：菜单条 31 / 状态栏 55=2×27+1px 顶描边 / 标签条 30 / 会话头 26）。
    const bool menu_ok = w.menuBar()->height() == m::kMenuBarHeight;
    const bool status_ok = w.statusBar()->height() == m::kStatusBarHeight &&
                           w.statusBar()->height() == 2 * m::kStatusRowHeight + 1;
    const bool tabbar_ok = w.tabs_bar()->height() == m::kTabBarHeight;
    SessionBanner *banner = w.page_banner(0);
    const bool banner_ok = banner != nullptr && banner->height() == m::kSessionHeaderHeight;
    ok &= menu_ok;
    check(menu_ok, QStringLiteral("C9-a 菜单栏高 %1").arg(m::kMenuBarHeight),
          QString::number(w.menuBar()->height()));
    ok &= status_ok;
    check(status_ok, QStringLiteral("C9-b 状态栏高 %1（=2×%2 + 1px 描边）")
                         .arg(m::kStatusBarHeight).arg(m::kStatusRowHeight),
          QString::number(w.statusBar()->height()));
    ok &= tabbar_ok;
    check(tabbar_ok, QStringLiteral("C9-c 标签条高 %1").arg(m::kTabBarHeight),
          QString::number(w.tabs_bar()->height()));
    ok &= banner_ok;
    check(banner_ok, QStringLiteral("C9-d 会话头高 %1").arg(m::kSessionHeaderHeight),
          banner ? QString::number(banner->height()) : QStringLiteral("无横幅"));

    // 2) 侧栏拖拽钳位 160–480（layout-manager.ts:58-59）。
    ChromeDocks *d = w.docks();
    const bool clamp_ok = d != nullptr && d->conn->minimumWidth() == m::kPanelWidthMin &&
                          d->conn->maximumWidth() == m::kPanelWidthMax &&
                          d->sess->minimumWidth() == m::kPanelWidthMin &&
                          d->sess->maximumWidth() == m::kPanelWidthMax;
    ok &= clamp_ok;
    check(clamp_ok, "C9-e 左右栏钳位 160–480");

    // 3) 会话头图标钮 22×22（session-header.css:31-51）。
    auto *btn = banner == nullptr ? nullptr : banner->findChild<QToolButton *>();
    const bool icon_ok = btn != nullptr && btn->width() == m::kSessionHeaderIconSize &&
                         btn->height() == m::kSessionHeaderIconSize;
    ok &= icon_ok;
    check(icon_ok, "C9-f 图标钮 22×22", btn ? QStringLiteral("%1×%2").arg(btn->width()).arg(btn->height())
                                            : QStringLiteral("未找到"));

    // 4) QSS 令牌覆盖：分隔条/提示/滚动条样式在表，且 alpha 令牌已转 rgba()。
    const QString qss = qApp->styleSheet();
    const bool qss_ok = qss.contains(QStringLiteral("QMainWindow::separator")) &&
                        qss.contains(QStringLiteral("QToolTip")) &&
                        qss.contains(QStringLiteral("rgba(")) &&
                        qss.contains(QStringLiteral("max-width: 220px"));
    ok &= qss_ok;
    check(qss_ok, "C9-g QSS 令牌覆盖（含 qApp 级）");

    // 5) 自绘件取色走主题（不得有字面量色）：accent 有效 + 状态色语义映射。
    const chrome::UiColors &c = chrome::ui_colors();
    const bool color_ok = c.accent.isValid() && c.status_connected.isValid() &&
                          chrome::status_color(QStringLiteral("connected")) == c.status_connected &&
                          chrome::status_color(QStringLiteral("connecting")) == c.warn &&
                          chrome::status_color(QStringLiteral("nonsense")) == c.text_disabled;
    ok &= color_ok;
    check(color_ok, "C9-h 主题取色/状态语义");

    // 6) 树行度量：delegate 行高走 kTreeRowHeight（WT 实测 20.2 → 21）。
    auto *tree = d == nullptr ? nullptr : qobject_cast<QTreeView *>(d->sess->widget());
    auto *del = tree == nullptr ? nullptr : dynamic_cast<QStyledItemDelegate *>(tree->itemDelegate());
    const bool row_ok = del != nullptr &&
                        del->sizeHint(QStyleOptionViewItem{}, QModelIndex{}).height() ==
                            m::kTreeRowHeight;
    ok &= row_ok;
    check(row_ok, QStringLiteral("C9-i 树行高 %1").arg(m::kTreeRowHeight));
    // 7) 连接状态、标签文本与两端留白。
    ok &= check_tab_status_layout(w);
    // 8) 终端正文配色与 chrome 同源：term_bg/term_fg = 引擎 base16[0]/[7]
    //    （两条推导路径各自独立：bridge/prefs POD vs term_ops::theme_base16）。
    const rust::Vec<uint8_t> probe = term_palette_probe();
    const bool term_pal_ok = probe.size() == 48 && c.term_bg.isValid() && c.term_fg.isValid() &&
                             c.term_bg != c.term_fg && c.term_bg.red() == probe[0] &&
                             c.term_bg.green() == probe[1] && c.term_bg.blue() == probe[2] &&
                             c.term_fg.red() == probe[21] && c.term_fg.green() == probe[22] &&
                             c.term_fg.blue() == probe[23];
    ok &= term_pal_ok;
    check(term_pal_ok, "C9-k 终端调色板/主题同源");

    // 9) 次级文字走 QSS 的 objectName 规则（内联 setStyleSheet 会在换主题时分叉）。
    //    2026-09-25：规则从 `[#name]`（Qt 不匹配）纠正为 ID 选择器 `#name`，断言随之更新。
    const bool muted_ok = qss.contains(QStringLiteral("QLabel#mutedText")) &&
                          qss.contains(QStringLiteral("QLabel#mutedHint"));
    ok &= muted_ok;
    check(muted_ok, "C9-l 次级文字令牌化");

    // 10) 状态栏**两行各自**按序排布不重叠（WT 批次 2：行 1 = 就绪 + fps/后端/主题，
    //     行 2 = 状态点/模式/窗口/行/字符/编码/时钟）。geometry 相对各自行容器，
    //     故跨行不可比，必须分行判。
    QCoreApplication::processEvents(); // 分段文本要经一轮布局才有真实几何
    static const char *kRow1Segs[] = {"statusMsg", "statusFps", "statusBackend", "statusTheme"};
    static const char *kRow2Segs[] = {"statusDot",  "statusKind",  "statusSize", "statusRows",
                                      "statusChars", "statusCharset", "statusRight"};
    auto in_order = [&w](const char *const *names, int n) {
        bool row_ok = true;
        int prev_right = -1;
        for (int i = 0; i < n; ++i) {
            auto *l = w.statusBar()->findChild<QLabel *>(QLatin1String(names[i]));
            const QRect g = l == nullptr ? QRect() : l->geometry();
            row_ok = row_ok && l != nullptr && g.width() > 0 && g.left() >= prev_right;
            prev_right = g.right() + 1;
        }
        return row_ok;
    };
    const bool order_ok = in_order(kRow1Segs, 4) && in_order(kRow2Segs, 7);
    ok &= order_ok;
    check(order_ok, "C9-m 状态栏两行分段不重叠");

    // The visible panel header is compact, regular-weight text like WindTerm.
    auto *head = dynamic_cast<chrome::PanelHeader *>(d->conn->titleBarWidget());
    auto *title = head ? head->findChild<QLabel *>(QStringLiteral("panelTitle")) : nullptr;
    const bool head_ok = title && title->font().pixelSize() == 12 &&
                         title->font().weight() == QFont::Normal &&
                         head->height() == m::kSectionHeaderHeight + m::kPanelFilterRowHeight;
    ok &= head_ok;
    check(head_ok, "C9-n panel header geometry and visible font");

    // 12) 标签「活动方块」与「状态点」取色语义不同（参考 center-area.css:86-118 vs
    //     130-156）：disconnected/error → 红，其余同状态点。
    const bool act_ok = chrome::activity_color(QStringLiteral("disconnected")) == c.danger &&
                        chrome::activity_color(QStringLiteral("error")) == c.danger &&
                        chrome::activity_color(QStringLiteral("connected")) == c.status_connected &&
                        chrome::activity_color(QStringLiteral("connecting")) == c.warn &&
                        chrome::status_color(QStringLiteral("disconnected")) ==
                            c.status_disconnected;
    ok &= act_ok;
    check(act_ok, "C9-o 活动方块 disconnected/error 转红");

    // 13) 关标签落左邻（参考 tabs[max(0, idx-1)]；Qt 默认落右邻）。
    w.add_tab();
    w.add_tab();
    const uint32_t keep = w.page_id(0);
    w.close_tab(1);
    const bool left_ok = w.page_count() == 2 && w.current_page() == 0 && w.page_id(0) == keep;
    ok &= left_ok;
    check(left_ok, "C9-p 关中间标签落左邻");
    ok &= check_two_tab_alignment(w);
    if (!png_out.empty()) {
        const QString path = QString::fromUtf8(png_out.data(), int(png_out.size()));
        w.grab().save(path);
        w.tabs_bar()->grab().save(path + QStringLiteral(".tabs.png"));
        w.page_banner(0)->grab().save(path + QStringLiteral(".header.png"));
    }
    // 14) 连接树右键八项（对齐批次 8 · §4.1）。模态框在 show_tree_menu 里，
    //     自检直接驱动「纯执行层」tree_action（arg = 对话框本该收集到的输入）。
    const QString sdir = QDir::temp().filePath(QStringLiteral("tideterm-c9-tree"));
    QDir(sdir).removeRecursively();
    QDir().mkpath(sdir);
    sess_test_store_dir(sdir.toUtf8().toStdString());
    auto qs = [](const rust::String &s) {
        return QString::fromUtf8(s.data(), int(s.size()));
    };
    auto row_count = [&qs]() { return int(sess_tree_rows().size()); };
    auto label_of = [&qs](const QString &id) {
        const auto rows = sess_tree_rows();
        for (const SessRow &r : rows) {
            if (qs(r.id) == id) {
                return qs(r.label);
            }
        }
        return QString();
    };
    auto has_label = [&qs](const QString &text) {
        const auto rows = sess_tree_rows();
        for (const SessRow &r : rows) {
            if (qs(r.label) == text) {
                return true;
            }
        }
        return false;
    };
    SessCfg pod = sess_form_get(rust::String(std::string()), 1);
    pod.name = rust::String("prod");
    pod.group_path = rust::String("生产");
    const rust::String saved = sess_save(pod);
    const QString tid = qs(saved);
    const bool seed_ok = !tid.isEmpty() && row_count() == 2; // 分组 + 会话
    check(seed_ok, "C9-q0 种子：1 分组 + 1 会话");
    ok &= seed_ok;

    // 提示语带的是新 id（不是名称），故「(副本)」后缀须从树里查。
    const bool dup_ok = chrome::tree_action(&w, tid, QStringLiteral("duplicate"))
                            .startsWith(i18n::tr("已复制为 %1").arg(QString())) &&
                        row_count() == 3 && label_of(tid) == QStringLiteral("prod") &&
                        has_label(QStringLiteral("prod (副本)"));
    ok &= dup_ok;
    check(dup_ok, "C9-q 复制：新 id + 名称后缀 (副本)，原条目不动");
    const bool ren_ok = chrome::tree_action(&w, tid, QStringLiteral("rename"),
                                            QStringLiteral("  ")).isEmpty() &&
                        label_of(tid) == QStringLiteral("prod") &&
                        chrome::tree_action(&w, tid, QStringLiteral("rename"),
                                            QStringLiteral("web"))
                            .contains(QStringLiteral("web")) &&
                        label_of(tid) == QStringLiteral("web");
    ok &= ren_ok;
    check(ren_ok, "C9-r 重命名：空名 = 取消，改名生效");
    const QString cur_grp = qs(sess_form_get(saved, 0).group_path);
    const bool mv_noop = chrome::tree_action(&w, tid, QStringLiteral("move"), cur_grp)
                             .isEmpty() &&
                         row_count() == 3; // 参考：与当前分组相同 = 不落盘
    const bool mv_ok = mv_noop &&
                       chrome::tree_action(&w, tid, QStringLiteral("move"),
                                           QStringLiteral(" 临时 "))
                           .contains(QStringLiteral("临时")) &&
                       row_count() == 4 && // +分组「临时」，「生产」仍装着副本
                       qs(sess_form_get(saved, 0).group_path) == QStringLiteral("临时");
    ok &= mv_ok;
    check(mv_ok, "C9-s 移动到分组：同组 no-op / 换组只多一行分组 / 去空白");
    const QString jpath = QDir(sdir).filePath(QStringLiteral("exp.json"));
    const bool exp_ok = chrome::tree_action(&w, tid, QStringLiteral("export"), jpath)
                            .startsWith(i18n::tr("已导出 %1").arg(QString())) &&
                        QFile::exists(jpath);
    ok &= exp_ok;
    check(exp_ok, "C9-t 导出：落盘 JSON");
    const bool imp_ok = chrome::tree_action(&w, QString(), QStringLiteral("import"), jpath)
                            .contains(i18n::tr("已导入 %1 个会话").arg(1)) &&
                        row_count() == 5; // 新 id 落在分组「临时」下
    ok &= imp_ok;
    check(imp_ok, "C9-u 导入：按新 id 落一条（不覆盖既有）");
    const QString bad = QDir(sdir).filePath(QStringLiteral("bad.json"));
    QFile bf(bad);
    (void)bf.open(QIODevice::WriteOnly);
    bf.write("{ nope");
    bf.close();
    const bool bad_ok = chrome::tree_action(&w, QString(), QStringLiteral("import"), bad)
                            .contains(i18n::tr("导入失败：文件格式错误或内容无效"));
    ok &= bad_ok;
    check(bad_ok, "C9-v 导入坏文件 = 参考 importError 文案");
    const bool del_ok = chrome::tree_action(&w, tid, QStringLiteral("delete"))
                            .contains(i18n::tr("已删除会话 %1").arg(tid));
    ok &= del_ok;
    check(del_ok, "C9-w 删除（确认后执行）");
    sess_test_store_reset();
    QDir(sdir).removeRecursively();

    // 15) 拖拽改组（对齐批次 9）：离屏没有鼠标 ⇒ 直接驱动模型的 mime 协议。
    const QString ddir = QDir::temp().filePath(QStringLiteral("tideterm-c9-drag"));
    QDir(ddir).removeRecursively();
    QDir().mkpath(ddir);
    sess_test_store_dir(ddir.toUtf8().toStdString());
    SessCfg seed = sess_form_get(rust::String(std::string()), 1);
    seed.name = rust::String("drag-me");
    seed.group_path = rust::String("");
    const QString did = qs(sess_save(seed));
    seed.name = rust::String("host");
    seed.group_path = rust::String("生产");
    const QString hid = qs(sess_save(seed));
    w.refresh_chrome();
    PodTreeModel *model = w.docks() == nullptr ? nullptr : w.docks()->tree_model;

    // 树序 = store 序：顶层 [drag-me(未分组), 分组「生产」]；按 id 找，别猜下标。
    auto find_by_id = [model](const QString &want, QModelIndex *out_parent) {
        std::function<QModelIndex(const QModelIndex &)> walk =
            [&](const QModelIndex &p) {
                const int n = model->rowCount(p);
                for (int i = 0; i < n; ++i) {
                    const QModelIndex c = model->index(i, 0, p);
                    if (c.data(PodTreeModel::RoleNodeId).toString() == want) {
                        if (out_parent != nullptr) {
                            *out_parent = p;
                        }
                        return c;
                    }
                    const QModelIndex deep = walk(c);
                    if (deep.isValid()) {
                        if (out_parent != nullptr) {
                            *out_parent = p;
                        }
                        return deep;
                    }
                }
                return QModelIndex();
            };
        return walk(QModelIndex());
    };
    const QModelIndex grp = find_by_id(QStringLiteral("group:生产"), nullptr);
    const QModelIndex src = find_by_id(did, nullptr);
    QString note;
    if (model != nullptr) {
        model->on_note = [&note](const QString &msg) { note = msg; };
    }
    const QMimeData *mime = model == nullptr ? nullptr : model->mimeData({src});
    const bool has_fmt = mime != nullptr && mime->hasFormat(QString::fromLatin1(kTreeDragMime));

    // 自己落到自己 = 拒（参考 dragState 也不允许 draggedId == targetId）。
    const bool self_reject = model != nullptr &&
                             !model->canDropMimeData(mime, Qt::MoveAction, -1, 0, src);
    const bool dropped = model != nullptr &&
                         model->dropMimeData(mime, Qt::MoveAction, -1, 0, grp);
    delete mime;
    if (model != nullptr) {
        model->on_note = nullptr;
    }
    w.refresh_chrome(); // 拖拽锁在 dropMimeData 里已解除，此处才重建
    const QString moved_grp = qs(sess_form_get(rs(did), 0).group_path);
    const QModelIndex grp2 = find_by_id(QStringLiteral("group:生产"), nullptr);
    const int kids = model == nullptr ? -1 : model->rowCount(grp2);
    const QModelIndex moved_item = find_by_id(did, nullptr);
    const bool drag_ok = has_fmt && self_reject && dropped &&
                         note.contains(i18n::tr("已移动会话 %1").arg(QString())) &&
                         moved_grp == QStringLiteral("生产") && moved_item.isValid() &&
                         moved_item.parent() == grp2 && kids == 2;
    ok &= drag_ok;
    check(drag_ok, "C9-x 拖入分组：改 group_path + UI 项归入目标组 + 自拖自落被拒");
    sess_test_store_reset();
    QDir(ddir).removeRecursively();
    // 16) 中键关闭 + 关闭确认（对齐批次 10）。模态框无法自动应答 ⇒ 注入确认策略。
    const int pages0 = w.page_count();
    int asked = 0;
    w.close_confirm_ask = [&asked](const QString &) {
        ++asked;
        return false; // 用户取消
    };
    auto *bar = w.tabs_bar();
    auto middle_click = [bar](QTabBar *t) {
        const QPoint c = t->tabRect(0).center();
        QMouseEvent ev(QEvent::MouseButtonRelease, QPointF(c), QPointF(t->mapToGlobal(c)),
                       Qt::MiddleButton, Qt::MiddleButton, Qt::NoModifier);
        QCoreApplication::sendEvent(t, &ev);
    };
    if (bar != nullptr) {
        middle_click(bar);
    }
    const bool cancel_ok = asked == 1 && w.page_count() == pages0;
    w.close_confirm_ask = [&asked](const QString &) {
        ++asked;
        return true; // 用户确认
    };
    if (bar != nullptr) {
        middle_click(bar);
    }
    const bool mid_ok = asked == 2 && bar != nullptr && w.page_count() == pages0 - 1;
    w.close_confirm_ask = nullptr;
    ok &= cancel_ok && mid_ok;
    check(cancel_ok && mid_ok, "C9-y 中键关闭：确认框被拒则不关，确认则关左邻");

    // 17) 窗格内边距 + 活动窗格 1px accent 描边（对齐批次 12 · §3 中央）。
    PtyView *pv = w.page_count() > 0 ? w.pane_view(0, 0) : nullptr;
    pv->resize(500, 300);
    QCoreApplication::processEvents();
    const bool pad_ok = selftest_ui::verify_history_geometry(pv);
    ok &= pad_ok;
    check(pad_ok, "C9-z 时间列/padding/历史条：正文范围排除真实滚动条，鼠标原点保持一致");

    const chrome::UiColors cols = chrome::ui_colors();
    pv->setFocus();
    w.activateWindow();
    QCoreApplication::processEvents();
    const QPixmap shot = pv->grab();
    const QImage img = shot.toImage();
    const QColor tl = img.pixelColor(0, 0);
    const QColor bl = img.pixelColor(0, img.height() - 1);
    // 2026-10-01：活动窗格 1px accent 描边按用户要求移除（WindTerm 同款无描边观感）
    // ⇒ 角像素应等于终端底色，且不得再出现 accent。
    const bool frame_ok = pv->hasFocus() && tl != cols.accent && bl != cols.accent &&
                          tl == cols.term_bg && bl == cols.term_bg;
    ok &= frame_ok;
    std::printf("[cpp] C9-aa focus=%d tl=%s bl=%s termBg=%s sz=%dx%d\n",
                int(pv->hasFocus()), tl.name(QColor::HexRgb).toUtf8().constData(),
                bl.name(QColor::HexRgb).toUtf8().constData(),
                cols.term_bg.name(QColor::HexRgb).toUtf8().constData(), img.width(),
                img.height());
    check(frame_ok, "C9-aa 活动窗格无描边（左上/左下像素 = 终端底色）");

    // 18) 会话头几何取 POD（term_stats），不再从 term_status 显示串 scrape。
    SessionBanner *bn = w.page_banner(0);
    const uint32_t tab0 = pv->tab();
    const TermStats ts = term_stats(tab0);
    bn->bind(tab0);
    const QString want =
        QStringLiteral("%1×%2 · UTF-8").arg(ts.cols).arg(ts.rows);
    const bool geom_ok = bn->grid_text() == want;
    ok &= geom_ok;
    std::printf("[cpp] C9-ab geom=%s want=%s cols=%u rows=%u\n",
                bn->grid_text().toUtf8().constData(), want.toUtf8().constData(),
                unsigned(ts.cols), unsigned(ts.rows));
    check(geom_ok, "C9-ab 会话头几何 = term_stats 真值（非字符串 scrape）");

    // 19) 分割比例钳 0.1–0.9（对齐批次 13 · §3 中央；参考 panel-split-manager.ts:197）。
    const bool clamp_math_ok = chrome::clamp_split_a(1000, 500) == 500 &&
                               chrome::clamp_split_a(1000, 5) == 100 &&
                               chrome::clamp_split_a(1000, 990) == 900 &&
                               chrome::clamp_split_a(0, 3) == 3;
    ok &= clamp_math_ok;
    check(clamp_math_ok, "C9-ac 比例钳纯函数：区间内不动、越界回钳、total<=0 不动");

    QSplitter sp(Qt::Horizontal);
    QWidget a(nullptr);
    QWidget b(nullptr);
    a.setMinimumSize(0, 0);
    b.setMinimumSize(0, 0);
    a.setMaximumSize(16777215, 16777215);
    b.setMaximumSize(16777215, 16777215);
    sp.addWidget(&a);
    sp.addWidget(&b);
    sp.resize(1000, 200);
    sp.setSizes({950, 49});
    const int total = sp.sizes()[0] + sp.sizes()[1];
    chrome::apply_split_ratio_clamp(&sp);
    const bool ratio_ok = sp.sizes()[0] == chrome::clamp_split_a(total, 950) &&
                          sp.sizes()[0] < 950 && sp.sizes()[0] + sp.sizes()[1] == total;
    ok &= ratio_ok;
    std::printf("[cpp] C9-ac total=%d clamped=%d\n", total, sp.sizes()[0]);
    check(ratio_ok, "C9-ac 越界分割经钳位回到 0.9 上界，且总量不变");

    QSplitter z(Qt::Horizontal); // 放大态豁免：兄弟被压到 0 时不得回拉
    z.setChildrenCollapsible(true);
    QWidget za(nullptr);
    QWidget zb(nullptr);
    z.addWidget(&za);
    z.addWidget(&zb);
    z.resize(1000, 200);
    z.setSizes({0, 1000});
    chrome::apply_split_ratio_clamp(&z);
    const bool zoom_ok = z.sizes()[0] == 0;
    ok &= zoom_ok;
    check(zoom_ok, "C9-ad 放大态（兄弟尺寸为 0）不参与钳位");

    ok &= check_screen_lock(w);
    std::printf("[cpp] 对齐批次 A/B 度量自检: %s\n", ok ? "通过" : "失败");
    return ok ? 0 : 1;
}

//! 主题 → QSS 生成（webview 版 `src/styles/**` 的 1:1 移植）。
//!
//! 从 `rust/theme.rs` 拆出：该文件同时承载 `theme_db` / `ui_prefs` 的 `#[path]`
//! 挂载点与 prefs 域桥实现，QSS 模板继续留在其中会顶到 500 行硬限。
//!
//! 口径：
//! - 颜色一律取自 `ThemeSpec` token，**不得出现字面量色值**（`css()` 负责
//!   `#RRGGBBAA` → `rgba()`，等价参考的 `color-mix` 预混在 `mix_alpha()`）。
//! - 只写颜色 / padding / 圆角 / 边框 / 字号；**高度**由 C++ `ui_metrics.h`
//!   统一定义，避免两处声明同一度量。
//! - 度量出处以 `docs/reports/qt-webview-ui-parity-2026-09-18.md` §2 为准。

use super::by_id;
use super::UiPrefs;

/// 主题 hex → QSS 颜色值：6 位原样 `#RRGGBB`；8 位（alpha-last）转
/// `rgba(r, g, b, a)`（Qt QSS 的 rgba alpha 走 0–255）。
pub fn css(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0);
    match h.len() {
        6 => format!("#{h}"),
        8 => format!("rgba({}, {}, {}, {})", byte(0), byte(1), byte(2), byte(3)),
        _ => "#000000".to_string(),
    }
}

/// 取主题色的 RGB 分量（8 位 hex 的 alpha 由调用方决定丢弃方式）。
fn rgb_of(hex: &str) -> (u16, u16, u16) {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0) as u16;
    (byte(0), byte(1), byte(2))
}

/// 等价 CSS `color-mix(in srgb, <token> <pct>%, transparent)`。QSS 无 `color-mix`，
/// 在 Rust 侧预混成 `rgba()`，alpha = pct（0–100）。
pub fn mix_alpha(hex: &str, pct: u8) -> String {
    let (r, g, b) = rgb_of(hex);
    format!("rgba({r}, {g}, {b}, {pct})")
}

/// 当前偏好的主题 → 全局 QSS。
pub fn qss_for(prefs: &UiPrefs) -> String {
    let t = by_id(&prefs.theme);
    let q = |tok: &str| css(tok);
    format!(
        /* ---- 全局底色/文字（PtyView 自绘不受影响，见 theme.rs 模块注释） ---- */
        "QWidget {{ background: {bg}; color: {text}; font-size: 13px; font-weight: 400; }} \
         QLabel, QCheckBox, QRadioButton, QGroupBox {{ background: transparent; }} \
         QDialog#sshLogin {{ border: 1px solid {bd1}; }} \
         QLabel#loginHeading {{ color: {ac}; font-size: 18px; padding: 0px 0px 8px; border-bottom: 1px solid {bd1}; }} \
         QLabel#loginError {{ color: {text_muted}; }} \
         QDialog#sshLogin QTabBar#loginModes::tab {{ border: none; border-radius: 0; \
             min-width: 0; max-width: 10000px; height: 24px; padding: 0 8px; background: {bg_root}; }} \
         QDialog#sshLogin QTabBar#loginModes::tab:selected {{ background: {bg_hover}; border: none; }} \
         QLabel#loginTrust {{ border: 1px solid {bd1}; padding: 2px 6px; color: {warn}; }} \
         QLabel#loginTrust[hostState=\"known\"] {{ color: {ok}; }} \
         QLabel#loginTrust[hostState=\"changed\"] {{ color: {danger}; }} \
         /* ---- 菜单栏：留出边距并提高文字可读性 ---- */ \
         QMenuBar {{ background: {bg_surface}; color: {text_secondary}; font-size: 13px; \
             padding-left: 4px; border-bottom: 1px solid {bd1}; }} \
         QMenuBar::item {{ background: transparent; padding: 4px 6px; border-radius: 0; \
             color: {text_secondary}; }} \
         QMenuBar::item:selected {{ background: {bg_hover}; color: {text}; }} \
         /* 页头左簇动作图标条（并入连接栏同一行，2026-10-01）：无外壳、贴行。 */ \
         QWidget#favoriteActions {{ background: transparent; }} \
         QWidget#favoriteActions QToolButton {{ background: transparent; border: none; \
             border-radius: 3px; padding: 0; }} \
         QWidget#favoriteActions QToolButton:hover {{ background: {bg_hover}; }} \
         QWidget#windowControls {{ background: transparent; }} \
         QWidget#windowControls QToolButton {{ border: none; border-radius: 0; padding: 0; \
             color: {text_secondary}; }} \
         QToolButton#windowClose:hover {{ background: {danger}; color: {text_on_accent}; }} \
         /* ---- 弹出菜单（WT 实测：行高 27 = 内容 17 + 上下各 5 内边距；图标槽靠图标画布） ---- */ \
         QMenu {{ background: {bg_elevated}; color: {text}; border: 1px solid {bd2}; \
             padding: 6px 8px; }} \
         QMenu::item {{ padding: 5px 24px 5px 18px; margin: 0; border-radius: 0; \
             min-height: 17px; }} \
         QMenu::item:selected {{ background: {ac}; color: {text}; }} \
         QMenu::item:disabled {{ color: {text_disabled}; }} \
         QMenu::separator {{ height: 1px; background: {bd3}; margin: 1px 8px; }} \
         QMenu::right-arrow {{ margin-right: 8px; }} \
         /* ---- 标签条（center-area.css:11-194） ---- */ \
         QTabWidget::pane {{ border: none; border-top: 1px solid {bd1}; }} \
         QTabWidget#termTabs::pane {{ border: none; }} \
         QTabBar {{ background: {bg_surface}; }} \
         /* 通用（叠放 dock 的标签条）：紧凑小页签，避免中央终端标签条的宽度约束影响侧栏。 */ \
         QTabBar::tab {{ background: {bg_surface}; color: {text_secondary}; \
             padding: 2px 8px; margin-right: 1px; border: none; }} \
         QTabBar::tab:selected {{ background: {bg_root}; color: {text}; }} \
         QTabBar::tab:hover:!selected {{ background: {bg_hover}; }} \
         /* 中央终端标签条（objectName=termTabs）：页签与 30px 标签条同高。 */ \
         QTabWidget#termTabs QTabBar::tab {{ background: {bg_surface}; \
             color: {text_secondary}; height: 30px; \
             min-width: 80px; max-width: 220px; \
             padding: 0 10px 0 8px; margin: 0 1px 0 0; \
             border: none; border-radius: 0; }} \
         QTabWidget#termTabs QTabBar::tab:selected {{ background: {bg_root}; color: {text}; \
             border: none; }} \
         QTabWidget#termTabs QTabBar::tab:hover:!selected {{ background: {bg_hover}; }} \
         QWidget#tabStatusLead, QWidget#tabCloseSlot {{ background: transparent; border: none; }} \
         QToolButton#tabClose {{ background: transparent; border: none; padding: 0; border-radius: 2px; }} \
         QToolButton#tabClose:hover {{ background: {bg_hover}; }} \
         /* 标签条右端快捷簇（WT `A` `+` `≡`）：18px 扁平钮，hover 提亮，图标色随主题。 */ \
         QWidget#tabQuickBar {{ background: transparent; }} \
         QToolButton#tabQuickBtn {{ background: transparent; border: none; \
             border-radius: 3px; color: {text_muted}; font-size: 13px; }} \
         QToolButton#tabQuickBtn:hover {{ background: {bg_hover}; color: {text}; }} \
         QToolButton#tabQuickBtn:pressed, QToolButton#tabQuickBtn:checked {{ \
             background: {bg_elevated}; }} \
         /* 带菜单的钮不给 QSS 默认小箭头（WT 的 A/≡ 无箭头；箭头在分数缩放下还会糊）。 */ \
         QToolButton#tabQuickBtn::menu-indicator {{ image: none; width: 0; height: 0; }} \
         /* 会话工具行左簇与 ⓘ/面包屑之间的 1px 竖分隔。 */ \
         QWidget#headerSep {{ background: {bd2}; }} \
         /* ---- Dock（side-panels.css:2-105：左右栏 bg-surface + 1px 分隔） ---- */ \
         QDockWidget {{ color: {text_secondary}; titlebar-close-icon: none; \
             titlebar-normal-icon: none; }} \
         /* 节头 = WT 面板头实测（物理 32 → 26）：高 26、padding 0 10；当前辅助字 12px/400 次级色、底边 bd-1。 */
         QDockWidget::title {{ background: {bg_surface}; padding: 0 10px; min-height: 26px; \
             border-bottom: 1px solid {bd1}; font-size: 12px; font-weight: 400; \
             color: {text_secondary}; }} \
         /* ---- 自绘面板头（PanelHeader 顶掉原生标题栏；WT 对齐批次 1） ---- */ \
         /* 用 ID 选择器（`#name`）——`[#name]` 那种属性写法 Qt 解析成「属性名 #name」不生效。 */ \
         QWidget#panelHeader {{ background: {bg_surface}; border-top: 1px solid {bd1}; \
             border-bottom: 1px solid {bd1}; }} \
         QLabel#panelDot, QLabel#panelDotAccent {{ background: {ac}; border-radius: 1px; }} \
         /* WT 逐面板异色（截图：资源管理器红 / 文件管理器·会话蓝 / 历史命令粉）；\
            我方 token 里没有粉，最近的是 warn。 */ \
         QLabel#panelDotDanger {{ background: {danger}; border-radius: 1px; }} \
         QLabel#panelDotWarn {{ background: {warn}; border-radius: 1px; }} \
         QLabel#panelDotInfo {{ background: {info}; border-radius: 1px; }} \
         QLabel#panelDotOk {{ background: {ok}; border-radius: 1px; }} \
         QLabel#panelTitle {{ color: {text_secondary}; font-size: 12px; font-weight: 400; }} \
         QToolButton#panelGear, QToolButton#panelClose {{ color: {text_muted}; \
             background: transparent; border: none; border-radius: 3px; font-size: 12px; }} \
         QToolButton#panelGear:hover, QToolButton#panelClose:hover {{ \
             background: {bg_hover}; color: {text}; }} \
         /* 筛选行：WT 面板头下沿一行，无边框（贴住面板头）。 */ \
         QLineEdit#panelFilter {{ background: transparent; border: none; border-radius: 0; \
             padding: 0 10px; font-size: 12px; color: {text}; }} \
         /* ---- 底栏发送面板（WT：tab 行 24 + 参数条 23 + 输入区） ---- */ \
         QWidget#sendTabs {{ background: {bg_surface}; border-top: 1px solid {bd1}; \
             border-bottom: 1px solid {bd1}; }} \
         QLabel#sendTabActive {{ background: {bg_root}; color: {text}; padding: 0 10px; \
             font-size: 12px; }} \
         QWidget#sendParams {{ background: {bg_surface}; border-bottom: 1px solid {bd1}; }} \
         QMainWindow::separator {{ width: 1px; height: 1px; background: {bd1}; }} \
         QMainWindow::separator:hover {{ background: {ac}; }} \
         /* ---- 树 / 列表（side-panels.css:187-321） ---- */ \
         QTreeView, QTreeWidget, QListView {{ background: {bg_surface}; \
             alternate-background-color: {bg_surface}; color: {text}; border: none; \
             selection-background-color: {ac_soft}; selection-color: {text}; \
             outline: none; }} \
         QTreeView::item, QTreeWidget::item, QListView::item {{ padding: 5px 8px; \
             border-radius: 3px; }} \
         QTreeView::item:hover, QListView::item:hover {{ background: {bg_hover}; }} \
         QTreeView::item:selected {{ background: {bg_hover}; color: {text}; }} \
         QHeaderView {{ background: transparent; }} \
         QHeaderView::section {{ background: {bg_surface}; color: {text_secondary}; \
             border: none; border-bottom: 1px solid {bd1}; padding: 4px 8px; \
             font-weight: 500; }} \
         /* ---- 工具条 / 工具钮（session-header.css:31-51、menu-bar.css:60-81） ---- */ \
         QToolBar {{ background: {bg_elevated}; border: none; border-bottom: 1px solid {bd1}; \
             spacing: 2px; padding: 4px 8px; }} \
         QToolButton {{ color: {text_secondary}; background: transparent; border: none; \
             border-radius: 2px; }} \
         QToolButton:hover {{ background: {bg_hover}; color: {text}; }} \
         QToolButton:pressed, QToolButton:checked {{ background: {bg_elevated}; }} \
         /* ---- 输入类（session-form.css:247-281、side-panels.css:150-177） ---- */ \
         QLineEdit, QPlainTextEdit, QSpinBox, QComboBox, QListWidget {{ \
             background: {bg_input}; color: {text}; border: 1px solid {bd2}; \
             border-radius: 3px; padding: 3px 6px; \
             selection-background-color: {ac}; }} \
         QLineEdit:focus, QPlainTextEdit:focus, QSpinBox:focus, QComboBox:focus {{ \
             border: 1px solid {ac}; }} \
         QLineEdit:disabled, QPlainTextEdit:disabled {{ color: {text_disabled}; }} \
         QComboBox::drop-down {{ border: none; width: 18px; }} \
         QComboBox QAbstractItemView {{ background: {bg_overlay}; color: {text}; \
             border: 1px solid {bd2}; selection-background-color: {bg_hover}; \
             selection-color: {text}; }} \
         /* ---- 按钮（session-form.css:575-607） ---- */ \
         QPushButton {{ background: {bg_overlay}; color: {text}; border: 1px solid {bd2}; \
             border-radius: 3px; padding: 5px 12px; }} \
         QPushButton:hover {{ background: {bg_hover}; border-color: {bd3}; }} \
         QPushButton:pressed {{ background: {bg_elevated}; }} \
         QPushButton:disabled {{ color: {text_disabled}; background: {bg_surface}; }} \
         QPushButton::menu-indicator {{ subcontrol-origin: padding; \
             subcontrol-position: right center; right: 4px; }} \
         /* ---- 状态栏（status-bar.css:2-42：高 24 由 ui_metrics 定） ---- */ \
         QStatusBar {{ background: {bg_surface}; color: {text_secondary}; \
             border-top: 1px solid {bd1}; font-size: 12px; }} \
         QStatusBar::item {{ border: none; }} \
         QStatusBar QLabel {{ color: {text_secondary}; font-size: 13px; }} \
         QFrame#statusSep {{ background: {bd2}; border: none; }} \
         /* ---- 滚动条（main.css:113-146：8px，thumb 带 2px 透明边） ---- */ \
         QScrollBar:vertical {{ background: transparent; width: 8px; margin: 0; }} \
         QScrollBar::handle:vertical {{ background: {bd3}; min-height: 24px; \
             border: 2px solid transparent; border-radius: 4px; }} \
         QScrollBar::handle:vertical:hover {{ background: {text_muted}; }} \
         QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {{ height: 0; }} \
         QScrollBar:horizontal {{ background: transparent; height: 8px; margin: 0; }} \
         QScrollBar::handle:horizontal {{ background: {bd3}; min-width: 24px; \
             border: 2px solid transparent; border-radius: 4px; }} \
         QScrollBar::handle:horizontal:hover {{ background: {text_muted}; }} \
         QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal {{ width: 0; }} \
         QScrollBar::add-page, QScrollBar::sub-page {{ background: transparent; }} \
         QScrollBar#fileScrollBar {{ background: transparent; border: none; }} \
         QScrollBar#fileScrollBar:vertical {{ width: 10px; }} \
         QScrollBar#fileScrollBar:horizontal {{ height: 10px; }} \
         QScrollBar#fileScrollBar::handle {{ background: {bd3}; border: 1px solid transparent; \
             border-radius: 3px; }} \
         QScrollBar#fileScrollBar::handle:hover, QScrollBar#fileScrollBar::handle:pressed {{ \
             background: {text_muted}; }} \
         /* ---- 分割线 / 工具提示 / 分组框 ---- */ \
         QSplitter::handle {{ background: {bd1}; }} \
         QSplitter::handle:hover {{ background: {ac}; }} \
         QToolTip {{ background: {bg_overlay}; color: {text}; border: 1px solid {bd2}; \
             padding: 3px 6px; border-radius: 3px; font-size: 12px; }} \
         QGroupBox {{ border: 1px solid {bd1}; border-radius: 3px; margin-top: 10px; \
             padding-top: 6px; color: {text_secondary}; }} \
         QGroupBox::title {{ subcontrol-origin: margin; left: 8px; padding: 0 4px; }} \
         /* ---- 语义状态色（status-bar.css:44-62、side-panels.css:285-303） ---- \
            注（2026-09-25 实测）：下面这些「按 objectName 选控件」的规则原写成 `[#name]`，\
            Qt 把它当「属性选择器里名为 #name 的属性」⇒ 一条都不匹配（实拍：面板色块不出现、\
            空态提示无次级色）。统一改成 ID 选择器 `#name`。 */ \
         QFrame#statusSep {{ background: {bd2}; border: none; }} \
         QLabel#statusDot {{ background: {status_disconnected}; border-radius: 4px; }} \
         QLabel#statusLive {{ background: {ok}; border-radius: 4px; }} \
         QLabel#dangerText {{ color: {danger}; }} \
         QLabel#warnText {{ color: {warn}; }} \
         QLabel#infoText {{ color: {info}; }} \
         QLabel#aiAccent {{ color: {ai}; }} \
         /* 次级/提示文字（webview .muted / .hint；替代 C++ 内联写死色值） \
            —— 内联 setStyleSheet 会让控件脱离全局 QSS，换主题时分叉。 */ \
         QLabel#mutedText {{ color: {text_secondary}; }} \
         QLabel#mutedHint {{ color: {text_muted}; font-size: 12px; }}",
        bg_root = q(t.bg),
        bg_surface = q(t.mantle),
        bg_elevated = q(t.bg_elevated),
        bg_overlay = q(t.surface2),
        bg_input = q(t.bg_input),
        bg_hover = q(t.surface),
        bg = q(t.bg),
        text = q(t.text),
        text_secondary = q(t.subtext),
        text_muted = q(t.faint),
        text_disabled = q(t.text_disabled),
        text_on_accent = q(t.text_on_accent),
        bd1 = q(t.border1),
        bd2 = q(t.border),
        bd3 = q(t.border3),
        ac = q(t.accent),
        ac_soft = mix_alpha(t.accent, 12),
        ai = q(t.ai),
        ok = q(t.ok),
        warn = q(t.warn),
        info = q(t.info),
        danger = q(t.danger),
        status_disconnected = q(t.status_disconnected),
    ) + &format!(
        "QWidget#sessionBanner {{ background: {bg}; }} \
         QWidget#sessionTarget {{ background: {bg_elevated}; border: none; border-radius: 0; }} \
         QWidget#sessionCrumb, QWidget#sessionAddressStack {{ background: transparent; border: none; }} \
         QLineEdit#sessionAddress {{ background: transparent; border: none; border-radius: 0; \
             padding: 0 5px; font-size: 13px; }} \
         QLineEdit#sessionAddress:focus {{ border: none; }} \
         /* 面包屑分段（2026-10-02 WT 取证：段可点，会话段弹同级会话列表） */ \
         QToolButton#crumbSeg {{ background: transparent; border: none; border-radius: 3px; \
             color: {text}; font-size: 13px; padding: 0 4px; }} \
         QToolButton#crumbSeg:hover {{ background: {bg_elevated}; }} \
         QLabel#crumbSep {{ color: {text_muted}; font-size: 12px; }} \
         QWidget#sendBar {{ background: {bg}; }} \
         QWidget#filePathRow {{ background: {bg}; border-bottom: 1px solid {bd1}; }} \
         QLineEdit#filePath {{ background: transparent; border: none; border-radius: 0; \
             padding: 0 6px; font-size: 13px; }} \
         QLineEdit#filePath:focus {{ border-bottom: 1px solid {ac}; }} \
         QToolButton#fileUp {{ background: transparent; border: none; padding: 0; }} \
         QToolBar#fileTools {{ background: transparent; border: none; padding: 0; spacing: 2px; }} \
         QToolBar#fileTools QToolButton {{ padding: 0; width: 20px; height: 20px; }} \
         QTreeWidget#fileTree QHeaderView::section {{ padding: 2px 6px; font-weight: 400; }} \
         QToolButton::menu-indicator {{ image: none; width: 0; height: 0; }} \
         QLineEdit#sendInput {{ background: transparent; border: none; padding: 2px 4px; }} \
         QPushButton#sendStart {{ background: transparent; border: none; border-radius: 3px; }} \
         QTabBar#sessionProtocols::tab {{ padding: 4px 0; margin: 0; border-radius: 0; border-right: 1px solid {bd1}; background: {bg_elevated}; }} \
         QTabBar#sessionProtocols::tab:selected {{ background: {bg}; border-bottom: none; color: {text}; }} \
         QTreeWidget#sessionSettingsNav {{ background: {bg}; border: 1px solid {bd1}; padding: 0; }} \
         QTreeWidget#sessionSettingsNav::item {{ padding: 3px 4px; border-radius: 0; }} \
         QTreeWidget#sessionSettingsNav::item:disabled {{ color: {text_disabled}; }} \
         QDialog#sessionForm QLineEdit, QDialog#sessionForm QComboBox, QDialog#sessionForm QSpinBox, \
         QDialog#sessionForm QPlainTextEdit {{ border-radius: 0; padding: 2px 5px; }} \
         QDialog#sessionForm QComboBox {{ padding-right: 18px; }} \
         QDialog#sessionForm QComboBox#sessionOneKey {{ padding-right: 40px; }} \
         QDialog#sessionForm QComboBox::down-arrow, QDialog#sessionForm QSpinBox::up-arrow, \
         QDialog#sessionForm QSpinBox::down-arrow {{ image: none; }} \
         QDialog#sessionForm QSpinBox {{ padding-right: 18px; }} \
         QDialog#sessionForm QSpinBox::up-button {{ subcontrol-origin: border; subcontrol-position: top right; \
             width: 16px; height: 11px; border: none; background: transparent; }} \
         QDialog#sessionForm QSpinBox::down-button {{ subcontrol-origin: border; subcontrol-position: bottom right; \
             width: 16px; height: 11px; border: none; background: transparent; }} \
         QWidget#sessionLabelRow {{ background: {bg_input}; border: 1px solid {bd1}; }} \
         QDialog#sessionForm QLineEdit#sessionLabel {{ border: none; background: transparent; }} \
         QLabel#systemBadge {{ background: {bg_elevated}; color: {text}; padding: 0 4px; }} \
         QToolButton#sendCtl {{ background: transparent; border: none; border-radius: 3px; \
             color: {text_muted}; }} \
         QToolButton#sendCtl:hover {{ background: {bg_hover}; color: {text}; }} \
         QWidget#sendParams QSpinBox, QWidget#sendParams QDoubleSpinBox, \
         QWidget#sendParams QComboBox {{ border-radius: 3px; padding: 0 4px; \
             background: {bg_input}; font-size: 12px; }} \
         QTreeView::item, QTreeWidget::item, QListView::item {{ padding: 1px 4px; border-radius: 0; }} \
         QListView#resourceList::item {{ padding: 3px 8px; }}",
        bg = q(t.bg),
        bg_input = q(t.bg_input),
        bg_elevated = q(t.bg_elevated),
        bg_hover = q(t.surface),
        text = q(t.text),
        text_muted = q(t.faint),
        text_disabled = q(t.text_disabled),
        ac = q(t.accent),
        bd1 = q(t.border1),
    )
}

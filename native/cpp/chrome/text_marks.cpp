#include "cpp/chrome/text_marks.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPixmap>
#include <QPointer>
#include <QPushButton>
#include <QTreeWidget>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::Str rs(const QByteArray &s) { return rust::Str(s.constData(), size_t(s.size())); }
QString color_name(int i) {
    const char *names[] = {"黄色", "绿色", "蓝色", "紫色", "橙色", "红色"};
    return i18n::tr(names[qBound(0, i, 5)]);
}
QIcon swatch(int i) { QPixmap pix(16, 16); pix.fill(chrome::mark_color(uint8_t(i))); return QIcon(pix); }
}

QColor chrome::mark_color(uint8_t color) {
    const QColor colors[] = {QColor(240, 196, 70), QColor(85, 196, 130), QColor(95, 166, 240),
        QColor(190, 130, 230), QColor(240, 150, 75), QColor(230, 105, 115)};
    return colors[qMin(int(color), 5)];
}
QString chrome::mark_error(uint8_t code) {
    switch (code) {
    case 0: return i18n::tr("标记规则已更新");
    case 1: return i18n::tr("规则为空、过长或包含换行，请修改后重试");
    case 2: return i18n::tr("正则表达式无效或超出复杂度限制");
    case 3: return i18n::tr("每个窗格最多保留 16 条标记规则");
    case 4: return i18n::tr("该规则已失效或不能继续移动");
    case 5: return i18n::tr("请选择一行连续文本，或将阅读光标移到单词上");
    case 6: return i18n::tr("锁定期间不能操作文本标记");
    case 7: return i18n::tr("标记来源已失效，请重新打开");
    case 8: return i18n::tr("默认规则读写失败，请检查配置目录或文件内容");
    case 9: return i18n::tr("请选择有效的标记颜色");
    case 10: return i18n::tr("相同的规则已存在，请修改已有项");
    case 11: return i18n::tr("默认标记已变化或多个窗格规则不一致。请手动保存需要的规则，或关闭自动保存后重试。");
    default: return i18n::tr("当前没有可导航的匹配");
    }
}
void chrome::paint_marks(QPainter &painter, uint32_t tab) {
    for (const auto &r : term_mark_rects(tab)) {
        QColor color = mark_color(r.color); color.setAlpha(78);
        painter.fillRect(QRectF(r.x, r.y, r.w, r.h), color);
    }
}
bool chrome::dispatch_marks(MainWindow *window, QAction *action) {
    const QByteArray id = action->property("tid").toByteArray();
    if (id == "search:highlight") { show_text_marks(window); return true; }
    if (id != "search:find-next-selection" && id != "search:find-previous-selection") return false;
    auto *view = window->active_view();
    if (!view || view->is_closed()) return true;
    const auto result = term_find_selection(view->tab(), id == "search:find-next-selection");
    window->note(result.error ? mark_error(result.error) : i18n::tr("已从选区查找匹配"));
    view->sync_reading_mode(); view->update(); window->refresh_chrome(); return true;
}

bool chrome::prepare_mark_close(MainWindow *window, const std::vector<PtyView *> &views) {
    std::vector<uint32_t> ids;
    for (auto *view : views) if (view && !view->is_closed()) ids.push_back(view->tab());
    if (ids.empty()) return true;
    const auto result = term_mark_prepare_close(rust::Slice<const uint32_t>(ids.data(), ids.size()));
    if (result.error) window->note(i18n::tr("未关闭：标记尚未保存。") + mark_error(result.error));
    return result.error == 0;
}

void chrome::show_text_marks(MainWindow *window) {
    QPointer<PtyView> view = window->active_view();
    if (!view || view->is_closed() || window->property("screenLocked").toBool()) return;
    const uint32_t tab = view->tab(); const auto generation = term_history_state(tab).generation;
    QDialog dialog(window); dialog.setObjectName(QStringLiteral("textMarks"));
    dialog.setWindowTitle(i18n::tr("文本标记")); dialog.resize(880, 640);
    auto *layout = new QVBoxLayout(&dialog);
    auto *scope = new QLabel(i18n::tr("仅作用于当前窗格的主屏；保存默认为以后新建的终端使用。"), &dialog);
    scope->setWordWrap(true); layout->addWidget(scope);
    auto *save_on_close = new QCheckBox(i18n::tr("关闭时保存标记为默认"), &dialog);
    save_on_close->setObjectName(QStringLiteral("markSaveOnClose"));
    save_on_close->setChecked(sess_active_config(tab).options.mark_save_on_close);
    save_on_close->setToolTip(i18n::tr("仅保存已修改的规则；冲突或写盘失败会保留窗格。保存会话可保留此设置。"));
    layout->addWidget(save_on_close);
    auto *quick = new QHBoxLayout();
    quick->addWidget(new QLabel(i18n::tr("快速标记选区或当前词"), &dialog));
    QList<QPushButton *> quick_buttons;
    for (int i = 0; i < 6; ++i) {
        auto *button = new QPushButton(swatch(i), color_name(i), &dialog);
        button->setObjectName(QStringLiteral("markQuick%1").arg(i)); quick_buttons << button; quick->addWidget(button);
    }
    auto *quick_remove = new QPushButton(i18n::tr("移除同文标记"), &dialog);
    quick_remove->setObjectName(QStringLiteral("markQuickRemove"));
    quick_remove->setToolTip(i18n::tr("移除相同文本、区分大小写的快速标记规则"));
    quick->addWidget(quick_remove); layout->addLayout(quick);
    auto *tree = new QTreeWidget(&dialog); tree->setObjectName(QStringLiteral("markRules"));
    tree->setHeaderLabels({i18n::tr("规则"), i18n::tr("匹配方式"), i18n::tr("颜色"), i18n::tr("状态")});
    tree->setRootIsDecorated(false); tree->setUniformRowHeights(true);
    tree->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    for (int i = 1; i < 4; ++i) tree->header()->setSectionResizeMode(i, QHeaderView::ResizeToContents);
    layout->addWidget(tree, 1);
    auto *pattern = new QLineEdit(&dialog); pattern->setObjectName(QStringLiteral("markPattern")); pattern->setMaxLength(512);
    auto *form = new QFormLayout(); form->addRow(i18n::tr("匹配文本"), pattern);
    auto *options = new QHBoxLayout();
    const auto check = [&](const char *label, const char *name, bool checked) {
        auto *box = new QCheckBox(i18n::tr(label), &dialog); box->setObjectName(QLatin1String(name)); box->setChecked(checked); options->addWidget(box); return box;
    };
    auto *match_case = check("区分大小写", "markCase", true);
    auto *regex = check("正则表达式", "markRegex", false);
    auto *word = check("全词匹配", "markWord", false);
    auto *enabled = check("启用", "markEnabled", true);
    auto *color = new QComboBox(&dialog); color->setObjectName(QStringLiteral("markColor"));
    for (int i = 0; i < 6; ++i) color->addItem(swatch(i), color_name(i));
    options->addWidget(color); form->addRow(options); layout->addLayout(form);
    auto *hint = new QLabel(i18n::tr("后面的规则优先；搜索和选择显示在标记之上。规则按物理行匹配。"), &dialog);
    hint->setWordWrap(true); layout->addWidget(hint);
    auto *status = new QLabel(&dialog); status->setObjectName(QStringLiteral("markStatus")); status->setTextFormat(Qt::PlainText); status->setWordWrap(true); layout->addWidget(status);
    const auto row = [&] { auto *value = new QHBoxLayout(); layout->addLayout(value); return value; };
    auto *actions = row();
    const auto button = [&](QHBoxLayout *box, const char *label, const char *name) {
        auto *b = new QPushButton(i18n::tr(label), &dialog); b->setObjectName(QLatin1String(name)); box->addWidget(b); return b;
    };
    auto *add = button(actions, "添加规则", "markAdd");
    auto *apply = button(actions, "应用修改", "markApply");
    auto *remove = button(actions, "移除规则", "markRemove");
    auto *fresh = button(actions, "新规则", "markNew");
    auto *up = button(actions, "上移", "markUp"); auto *down = button(actions, "下移", "markDown");
    auto *previous = button(actions, "上一匹配", "markPrevious"); auto *next = button(actions, "下一匹配", "markNext");
    auto *footer = row();
    auto *save = button(footer, "保存为默认", "markSave");
    auto *load = button(footer, "载入默认", "markLoad"); load->setToolTip(i18n::tr("载入默认会替换当前窗格的规则"));
    auto *clear = button(footer, "清除全部规则", "markClear"); auto *refresh = button(footer, "刷新", "markRefresh");
    footer->addStretch(); auto *close = button(footer, "关闭", "markClose");
    const auto ready = [&] { const auto state = term_history_state(tab); return view && !view->is_closed() && !window->property("screenLocked").toBool() && state.available && state.generation == generation; };
    const auto selected = [&]() -> uint64_t { return tree->currentItem() ? tree->currentItem()->data(0, Qt::UserRole).toULongLong() : 0; };
    const auto update_buttons = [&] {
        const bool valid = ready(); const bool chosen = valid && selected();
        add->setDefault(!chosen); apply->setDefault(chosen);
        save_on_close->setEnabled(valid);
        const auto seed = valid ? term_mark_seed(tab, false).error : uint8_t(7);
        for (auto *b : quick_buttons) b->setEnabled(valid && seed == 0);
        quick_remove->setEnabled(valid && seed == 0);
        for (auto *b : {add, fresh, save, load, clear}) b->setEnabled(valid);
        for (auto *b : {apply, remove, up, down, previous, next}) b->setEnabled(chosen);
        pattern->setEnabled(valid); match_case->setEnabled(valid); regex->setEnabled(valid); word->setEnabled(valid); enabled->setEnabled(valid); color->setEnabled(valid);
        if (!valid) status->setText(mark_error(7));
    };
    const auto edit = [&] {
        if (const uint64_t id = selected()) for (const auto &r : term_mark_rows(tab)) if (r.id == id) {
            pattern->setText(q(r.pattern)); match_case->setChecked(r.flags & 1); regex->setChecked(r.flags & 2); word->setChecked(r.flags & 4); color->setCurrentIndex(r.color); enabled->setChecked(r.enabled); break;
        }
        update_buttons();
    };
    const auto reload = [&](uint64_t wanted) {
        tree->clear();
        if (ready()) for (const auto &r : term_mark_rows(tab)) {
            auto *item = new QTreeWidgetItem(tree); item->setData(0, Qt::UserRole, QVariant::fromValue(qulonglong(r.id)));
            item->setText(0, q(r.pattern)); item->setToolTip(0, item->text(0));
            QStringList flags; if (r.flags & 1) flags << i18n::tr("区分大小写"); if (r.flags & 2) flags << i18n::tr("正则表达式"); if (r.flags & 4) flags << i18n::tr("全词匹配");
            item->setText(1, flags.isEmpty() ? i18n::tr("文本") : flags.join(QStringLiteral(" / ")));
            item->setText(2, color_name(r.color)); item->setIcon(2, swatch(r.color)); item->setText(3, r.enabled ? i18n::tr("已启用") : i18n::tr("已停用"));
            if (r.id == wanted) tree->setCurrentItem(item);
        }
        update_buttons();
    };
    const auto done = [&](MarkResult result, const QString &success = QString()) {
        status->setText(result.error ? mark_error(result.error) : (success.isEmpty() ? mark_error(0) : success));
        if (!result.error && view) { view->sync_reading_mode(); view->update(); window->refresh_chrome(); reload(result.id); }
        update_buttons();
    };
    QObject::connect(save_on_close, &QCheckBox::toggled, &dialog, [&](bool on) {
        if (ready()) {
            const auto result = term_mark_set_save_on_close(tab, on);
            status->setText(result.error ? mark_error(result.error) : i18n::tr("保存方式已更新"));
        }
        update_buttons();
    });
    QObject::connect(tree, &QTreeWidget::currentItemChanged, &dialog, edit);
    QObject::connect(add, &QPushButton::clicked, &dialog, [&] {
        if (ready()) done(term_mark_put(tab, 0, rs(pattern->text().toUtf8()), uint32_t(match_case->isChecked()) | (uint32_t(regex->isChecked()) << 1) | (uint32_t(word->isChecked()) << 2), uint8_t(color->currentIndex()), enabled->isChecked())); else update_buttons();
    });
    QObject::connect(apply, &QPushButton::clicked, &dialog, [&] {
        if (ready()) done(term_mark_put(tab, selected(), rs(pattern->text().toUtf8()), uint32_t(match_case->isChecked()) | (uint32_t(regex->isChecked()) << 1) | (uint32_t(word->isChecked()) << 2), uint8_t(color->currentIndex()), enabled->isChecked())); else update_buttons();
    });
    QObject::connect(fresh, &QPushButton::clicked, &dialog, [&] { tree->setCurrentItem(nullptr); pattern->clear(); match_case->setChecked(true); regex->setChecked(false); word->setChecked(false); enabled->setChecked(true); pattern->setFocus(); update_buttons(); });
    QObject::connect(remove, &QPushButton::clicked, &dialog, [&] { if (ready()) done(term_mark_remove(tab, selected())); else update_buttons(); });
    QObject::connect(clear, &QPushButton::clicked, &dialog, [&] { if (ready()) done(term_mark_clear(tab)); else update_buttons(); });
    for (int i = 0; i < 6; ++i) QObject::connect(quick_buttons[i], &QPushButton::clicked, &dialog, [&, i] { if (ready()) done(term_mark_quick(tab, uint8_t(i), false)); else update_buttons(); });
    QObject::connect(quick_remove, &QPushButton::clicked, &dialog, [&] { if (ready()) done(term_mark_quick(tab, 0, true)); else update_buttons(); });
    QObject::connect(up, &QPushButton::clicked, &dialog, [&] { if (ready()) done(term_mark_move(tab, selected(), true)); else update_buttons(); });
    QObject::connect(down, &QPushButton::clicked, &dialog, [&] { if (ready()) done(term_mark_move(tab, selected(), false)); else update_buttons(); });
    for (auto *b : {previous, next}) QObject::connect(b, &QPushButton::clicked, &dialog, [&, b] { if (ready()) done(term_mark_step(tab, selected(), b == next)); else update_buttons(); });
    QObject::connect(save, &QPushButton::clicked, &dialog, [&] { if (ready()) done(term_mark_defaults(tab, true), i18n::tr("默认规则已保存；现有其他窗格保持各自规则")); else update_buttons(); });
    QObject::connect(load, &QPushButton::clicked, &dialog, [&] { if (ready()) done(term_mark_defaults(tab, false)); else update_buttons(); });
    QObject::connect(refresh, &QPushButton::clicked, &dialog, [&] { status->clear(); reload(selected()); });
    QObject::connect(close, &QPushButton::clicked, &dialog, &QDialog::accept);
    QObject::connect(view, &QObject::destroyed, &dialog, &QDialog::reject);
    reload(0); const auto seed = term_mark_seed(tab, false); if (!seed.error) pattern->setText(q(seed.text));
    pattern->setFocus(); dialog.exec();
}

#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/tab_quick_bar.h"
#include "cpp/chrome/tab_close.h"
#include "cpp/chrome/split_ratio.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/main_window.h"
#include <QSignalBlocker>
#include <QSplitter>
#include <QTabWidget>
#include <QTabBar>
#include <QToolButton>
#include <QUuid>
#include <QVBoxLayout>
#include <QPainter>
#include <QFontMetricsF>
#include <QStyleOptionTab>

namespace {
class TerminalTabBar final : public QTabBar {
public:
    explicit TerminalTabBar(QWidget *parent) : QTabBar(parent) {}
protected:
    void tabLayoutChange() override {
        QTabBar::tabLayoutChange();
        // Center symbols against visible glyphs, not the font's ascent/descent box.
        // Keep Qt's own text painting, elision, scrolling and drag feedback.
        for (int at = 0; at < count(); ++at) {
            QStyleOptionTab option;
            initStyleOption(&option, at);
            const QRect text_box = style()->subElementRect(QStyle::SE_TabBarTabText, &option, this);
            const QFontMetricsF metrics(font(), this);
            const QString text = metrics.elidedText(option.text, elideMode(), text_box.width());
            const QRectF ink = metrics.tightBoundingRect(text);
            const qreal baseline = text_box.y() + (text_box.height() - metrics.height()) / 2 + metrics.ascent();
            const qreal center = ink.isEmpty() ? QRectF(option.rect).center().y() : baseline + ink.center().y();
            for (const auto side : {LeftSide, RightSide})
                if (auto *button = tabButton(at, side))
                    button->move(button->x(), qRound(center - button->height() / 2.0));
        }
    }
    void paintEvent(QPaintEvent *event) override {
        QTabBar::paintEvent(event);
        // Paint the indicator over the row; a top border shifts only Qt's text box.
        if (currentIndex() < 0) return;
        QPainter painter(this);
        const QRect tab = tabRect(currentIndex());
        painter.fillRect(QRect(tab.x(), tab.y(), tab.width(), 2), chrome::ui_colors().accent);
    }
};
class TerminalTabGroup final : public QTabWidget {
public:
    TerminalTabGroup() { setTabBar(new TerminalTabBar(this)); }
};
} // namespace

TabWorkspace::TabWorkspace(MainWindow *window) : QWidget(window), window_(window) {
    token_ = QUuid::createUuid().toByteArray();
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    root_ = new QSplitter(Qt::Horizontal, this);
    chrome::install_split_ratio(root_);
    layout->addWidget(root_);
    root_->addWidget(create_group());
}

QTabWidget *TabWorkspace::create_group() {
    auto *group = new TerminalTabGroup;
    group->setObjectName(QStringLiteral("termTabs"));
    group->setMinimumSize(160, 160);
    group->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
    QString name;
    for (int at = 0; at < 26; ++at) {
        name = QString(QChar('A' + at));
        bool used = false;
        for (auto *existing : groups_) used |= group_name(existing) == name;
        if (!used) break;
    }
    group->setProperty("groupName", name);
    group->setTabsClosable(true);
    group->setMovable(true);
    group->tabBar()->setSelectionBehaviorOnRemove(QTabBar::SelectLeftTab);
    group->setAcceptDrops(true);
    group->tabBar()->setAcceptDrops(true);
    group->tabBar()->setFixedHeight(chrome::metrics::kTabBarHeight);
    group->installEventFilter(this);
    group->tabBar()->installEventFilter(this);
    groups_.push_back(group);
    group->setCornerWidget(chrome::build_tab_quick_bar(window_, group));
    for (auto *button : group->cornerWidget()->findChildren<QToolButton *>())
        if (button->property("groupHandle").toBool()) install_group_drag(button, group);
    connect(group, &QTabWidget::tabCloseRequested, this, [this, group](int at) {
        if (on_close) on_close(group->widget(at));
    });
    chrome::install_middle_click_close(group->tabBar(), [this, group](int at) {
        if (on_close) on_close(group->widget(at));
    });
    connect(group->tabBar(), &QTabBar::tabMoved, this, [this, group](int, int) {
        if (on_reorder) on_reorder(group);
    });
    connect(group, &QTabWidget::currentChanged, this, [this](int) {
        if (on_changed) on_changed();
    });
    return group;
}

QTabWidget *TabWorkspace::first_group() const { return groups_.value(0); }

QTabWidget *TabWorkspace::owner(QWidget *page) const {
    for (auto *group : groups_) if (group->indexOf(page) >= 0) return group;
    return nullptr;
}

QString TabWorkspace::group_name(QTabWidget *group) const {
    return group ? group->property("groupName").toString() : QString();
}

void TabWorkspace::activate(QTabWidget *group) {
    if (groups_.contains(group) && on_active) on_active(group);
}

QTabWidget *TabWorkspace::split_group(QTabWidget *source, Qt::Orientation orientation, bool leading) {
    if (!groups_.contains(source) || groups_.size() >= 26) return nullptr;
    auto *parent = dynamic_cast<QSplitter *>(source->parentWidget());
    if (!parent) return nullptr;
    const QSize previous = source->size();
    auto *nested = new QSplitter(orientation);
    chrome::install_split_ratio(nested);
    parent->replaceWidget(parent->indexOf(source), nested);
    auto *created = create_group();
    if (leading) { nested->addWidget(created); nested->addWidget(source); }
    else { nested->addWidget(source); nested->addWidget(created); }
    const int span = orientation == Qt::Horizontal ? previous.width() : previous.height();
    nested->setSizes({qMax(1, span / 2), qMax(1, span / 2)});
    nested->show(); source->show(); created->show();
    return created;
}

void TabWorkspace::move_page(QWidget *page, QTabWidget *target, int at) {
    auto *source = owner(page);
    if (!source || !groups_.contains(target)) return;
    if (source == target) {
        if (at >= 0 && at < target->count()) target->tabBar()->moveTab(target->indexOf(page), at);
        target->setCurrentWidget(page); activate(target); return;
    }
    const int old = source->indexOf(page);
    const QString title = source->tabText(old);
    const QIcon icon = source->tabIcon(old);
    {
        QSignalBlocker block_source(source), block_target(target);
        source->removeTab(old);
        const int added = target->insertTab(at < 0 ? target->count() : at, page, icon, title);
        target->setCurrentIndex(added);
    }
    page->show();
    activate(target);
    prune_empty(source);
    if (on_changed) on_changed();
}

void TabWorkspace::merge_group(QTabWidget *source, QTabWidget *target) {
    if (source == target || !groups_.contains(source) || !groups_.contains(target)) return;
    QList<QWidget *> pages;
    for (int i = 0; i < source->count(); ++i) pages.push_back(source->widget(i));
    QWidget *selected = source->currentWidget();
    for (auto *page : pages) move_page(page, target);
    if (selected) target->setCurrentWidget(selected);
    activate(target);
}

void TabWorkspace::collapse(QSplitter *splitter) {
    if (!splitter || splitter == root_ || splitter->count() != 1) return;
    auto *parent = dynamic_cast<QSplitter *>(splitter->parentWidget());
    if (!parent) return;
    auto *only = splitter->widget(0);
    const int at = parent->indexOf(splitter);
    only->hide(); only->setParent(nullptr);
    parent->replaceWidget(at, only);
    only->show(); splitter->deleteLater();
    auto sizes = parent->sizes();
    for (int &size : sizes) size = qMax(1, size);
    parent->setSizes(sizes);
    collapse(parent);
}

void TabWorkspace::prune_empty(QTabWidget *group) {
    if (!groups_.contains(group) || group->count() || groups_.size() <= 1) return;
    auto *parent = dynamic_cast<QSplitter *>(group->parentWidget());
    groups_.removeAll(group);
    group->hide(); group->setParent(nullptr); group->deleteLater();
    collapse(parent);
    if (on_changed) on_changed();
}

void TabWorkspace::install_group_drag(QToolButton *button, QTabWidget *group) {
    button->setProperty("dragTabGroup", QVariant::fromValue<QObject *>(group));
    button->installEventFilter(this);
}

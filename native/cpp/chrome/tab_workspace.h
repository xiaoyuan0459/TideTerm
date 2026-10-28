// UI-only ownership of independent tab groups. Moving pages preserves sessions.
#pragma once
#include <QWidget>
#include <QPointer>
#include <QList>
#include <QByteArray>
#include <QJsonObject>
#include <functional>

class QTabWidget;
class QSplitter;
class QMimeData;
class QToolButton;
class MainWindow;

class TabWorkspace final : public QWidget {
public:
    explicit TabWorkspace(MainWindow *window);
    QTabWidget *first_group() const;
    QList<QTabWidget *> groups() const { return groups_; }
    QTabWidget *owner(QWidget *page) const;
    QTabWidget *split_group(QTabWidget *source, Qt::Orientation orientation, bool leading = false);
    void move_page(QWidget *page, QTabWidget *target, int at = -1);
    void merge_group(QTabWidget *source, QTabWidget *target);
    void prune_empty(QTabWidget *group);
    void install_group_drag(QToolButton *button, QTabWidget *group);
    QString group_name(QTabWidget *group) const;
    void activate(QTabWidget *group);
    QSplitter *root_splitter() const { return root_; }
    bool restore_tree(const QJsonObject &tree, const std::function<QWidget *(const QJsonObject &)> &page);
    std::function<void(QTabWidget *)> on_active;
    std::function<void(QWidget *)> on_close;
    std::function<void(QTabWidget *)> on_reorder;
    std::function<void()> on_changed;

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    QTabWidget *create_group();
    void collapse(QSplitter *splitter);
    void start_drag(QTabWidget *group, QWidget *page, QWidget *handle);
    bool decode_drag(const QMimeData *mime, QTabWidget **source, QWidget **page) const;
    void finish_drop(const QMimeData *mime, QTabWidget *target, const QPoint &position, bool tab_bar);
    MainWindow *window_;
    QSplitter *root_;
    QList<QTabWidget *> groups_;
    QByteArray token_;
    QPointer<QWidget> pressed_handle_;
    QPointer<QTabWidget> pressed_group_;
    QPointer<QWidget> pressed_page_;
    QPoint pressed_position_;
};

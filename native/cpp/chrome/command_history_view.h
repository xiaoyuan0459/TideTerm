#pragma once
#include <QWidget>
#include <QStringList>
#include <cstdint>
class MainWindow;
class QListView;
class QStringListModel;
class QLabel;

class CommandHistoryView final : public QWidget {
public:
    explicit CommandHistoryView(MainWindow *window);
    void refresh(uint32_t tab);
    void set_filter(const QString &text);
private:
    void apply_filter();
    MainWindow *window_;
    QListView *list_;
    QStringListModel *model_;
    QLabel *empty_;
    uint32_t tab_ = UINT32_MAX;
    uint64_t revision_ = UINT64_MAX;
    QString filter_;
    QStringList commands_;
};

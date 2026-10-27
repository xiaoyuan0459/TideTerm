#pragma once
#include <QColor>
#include <QString>
#include <cstdint>
#include <vector>
class PtyView;
class QAction;
class MainWindow;
class QPainter;
namespace chrome {
QColor mark_color(uint8_t color);
QString mark_error(uint8_t code);
void paint_marks(QPainter &painter, uint32_t tab);
bool dispatch_marks(MainWindow *window, QAction *action);
void show_text_marks(MainWindow *window);
bool prepare_mark_close(MainWindow *window, const std::vector<PtyView *> &views);
}

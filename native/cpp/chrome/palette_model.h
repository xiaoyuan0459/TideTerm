#pragma once
#include <cstdint>
#include <QByteArray>
#include <QPointer>
#include <QString>
#include <QVector>
class QAction;
class MainWindow;
class PtyView;
namespace chrome {
enum class PaletteMode { Actions, Lines, Symbols, Sessions };
enum class PaletteKind { Action, Mode, Symbol, Live, Saved, Line };
struct PaletteItem {
    PaletteKind kind = PaletteKind::Action;
    PaletteMode mode = PaletteMode::Actions;
    QString key, title, detail, shortcut, saved, unavailable;
    QByteArray tid;
    QPointer<QAction> action;
    QList<QPointer<QAction>> parents;
    QPointer<PtyView> view;
    uint64_t command = 0;
};
struct PaletteModel { QVector<PaletteItem> items; bool truncated = false; uint8_t error = 0; };
PaletteModel palette_model(MainWindow *window, PtyView *source, PaletteMode mode);
bool palette_action_ready(const PaletteItem &item);
bool palette_live_ready(MainWindow *window, const PaletteItem &item);
}

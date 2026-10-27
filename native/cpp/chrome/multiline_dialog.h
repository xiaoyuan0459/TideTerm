#pragma once
#include <QString>
#include <cstdint>
#include <vector>
class MainWindow;
class PtyView;
class QWidget;
namespace chrome {
void paste_clipboard(PtyView *view);
void show_multiline(MainWindow *window, PtyView *source, const QString &text,
                    uint8_t paste = 0, int scope = 0,
                    const std::vector<uint32_t> &targets = {}, QWidget *draft_owner = nullptr);
}

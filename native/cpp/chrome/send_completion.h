#pragma once
#include <functional>
#include <cstdint>
class QHBoxLayout;
class QLineEdit;
class QWidget;
class QVBoxLayout;
struct CompletionContext;
namespace chrome {
void install_send_completion(QWidget *owner, QHBoxLayout *row, QLineEdit *input,
                             std::function<uint32_t()> source, std::function<bool()> enabled);
void edit_user_commands(QWidget *owner);
void install_send_quickbar(QWidget *owner, QVBoxLayout *layout, QLineEdit *input,
                          std::function<uint32_t()> source, std::function<bool()> enabled);
void refresh_send_quickbar(QWidget *owner);
bool fill_user_command(QWidget *owner, QLineEdit *input, const CompletionContext &context, uint32_t id,
                       const std::function<bool()> &valid);
}

#pragma once

class QWidget;
namespace chrome {
/// 在构造控件前设置可显示中英文的 UI 字体。
void init_app_font();
void set_ui_text_font(QWidget *widget);
void set_connection_font(QWidget *widget);
}

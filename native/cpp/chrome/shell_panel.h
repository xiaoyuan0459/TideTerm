// Independent auxiliary terminals share the existing Rust session registry.
#pragma once
#include <QTabWidget>
#include <cstdint>
class MainWindow;
class PtyView;

class ShellPanel final : public QTabWidget {
public:
    explicit ShellPanel(MainWindow *window);
    ~ShellPanel() override;
    void ensure_shell();
    bool poll(bool titles);
    void close_shell(int index);
    bool focus_session(uint32_t id);
    bool owns_focus() const;
    PtyView *active_view() const;
private:
    void open_shell(const QString &name);
    MainWindow *window_;
};

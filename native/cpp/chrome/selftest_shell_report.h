#pragma once
#include <cstdint>
class MainWindow;
class SendBar;
class QString;
namespace selftest_ui { bool verify_shell_report(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png); }
namespace selftest_ui { bool verify_names(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png); }
namespace selftest_ui { bool verify_git_refs(MainWindow &window, SendBar *bar, uint32_t tab, const QString &directory, const QString &png); }
namespace selftest_ui { bool verify_docker(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png); }
namespace selftest_ui { bool verify_kube(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png); }

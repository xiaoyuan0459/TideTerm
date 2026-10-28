#pragma once
#include "cpp/bridge_shim.h"
class QWidget;

// Compose a transient SSH login. Cancellation never saves or opens a session.
bool run_login_dialog(QWidget *parent, const QuickTarget &target, SessCfg *cfg);

// 桥拆域 spike 的 C++ 声明头（ADR-014 §Decision 3）。
// 各域各生成头：本域声明在 native/src/bridge/scaffold.rs.h（cxx 生成）。
// 双处声明规则不变：C++ 侧新增函数需在桥（Rust）+ 本头文件同名列出。
#pragma once

#include <cstdint>

// cxx 为 src/bridge/scaffold.rs 生成的声明头（含 rust::… 与本桥共享 POD）。
#include "native/src/bridge/scaffold.rs.h"

/// C++ 侧透传：返回 spike_rust_id() + v（cxx 把 extern "Rust" 声明在全局作用域）。
uint32_t qt_spike_passthrough(uint32_t v);

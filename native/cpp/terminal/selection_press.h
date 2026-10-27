// Pointer routing for a left press: extend a live selection, or start a new one.
#pragma once

#include "cpp/bridge_shim.h"
#include <Qt>

/// Shift（可叠加 Alt）把落点当作「选择到」的终点：先试着只重瞄既有选区的一端，没有可扩展的
/// 选区时才退回新建。返回 true 表示这次按压扩展了选区，调用方不应再新建。
///
/// 选择状态由 Rust 的 TerminalPane 持有，这里只决定意图并转发；Alt 让这一次延伸成为矩形，
/// 与 Alt 拖拽的临时列选一致，且不会改变窗格默认模式。
inline bool extend_or_begin_selection(uint32_t tab, const QPointF &p, Qt::KeyboardModifiers mods,
                                      bool column) {
    if (term_selection_locked(tab)) return false;
    if (mods & Qt::ControlModifier)
        return term_selection_begin_add(tab, float(p.x()), float(p.y()), column);
    if ((mods & Qt::ShiftModifier) &&
        term_selection_extend_to(tab, float(p.x()), float(p.y()),
                                 (mods & Qt::AltModifier) != 0))
        return true;
    if (term_multi_selection(tab))
        return term_selection_begin_add(tab, float(p.x()), float(p.y()), column);
    if (column)
        term_selection_begin_column(tab, float(p.x()), float(p.y()));
    else
        term_selection_begin(tab, float(p.x()), float(p.y()));
    return true;
}
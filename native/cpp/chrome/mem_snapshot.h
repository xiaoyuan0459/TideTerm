// mem_snapshot —— 进程内存构成取证（只在内存探针路径调用，生产不触发）。
#pragma once

namespace chrome {

/// 打印地址空间区段图 + 各堆存活块直方图。tag 标注本次读数的口径。
void dump_mem_snapshot(const char *tag);

} // namespace chrome

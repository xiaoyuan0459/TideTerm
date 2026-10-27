#include "cpp/chrome/split_ratio.h"

#include <QList>
#include <QObject>
#include <QSplitter>

#include "cpp/chrome/ui_metrics.h"

namespace chrome {

int clamp_split_a(int total, int a) {
    if (total <= 0) {
        return a;
    }
    const int lo = int(double(total) * metrics::kSplitRatioMin);
    const int hi = int(double(total) * metrics::kSplitRatioMax);
    if (lo >= hi) { // 极窄窗格：区间退化，别把两侧互相顶开
        return a;
    }
    return a < lo ? lo : (a > hi ? hi : a);
}

void apply_split_ratio_clamp(QSplitter *sp) {
    const QList<int> s = sp->sizes();
    if (s.size() != 2 || s[0] == 0 || s[1] == 0) {
        return; // 非二元节点（放大态把兄弟压到 0）不参与钳位
    }
    const int total = s[0] + s[1];
    const int clamped = clamp_split_a(total, s[0]);
    if (clamped != s[0]) {
        sp->setSizes({clamped, total - clamped});
    }
}

void install_split_ratio(QSplitter *sp) {
    sp->setChildrenCollapsible(false);
    QObject::connect(sp, &QSplitter::splitterMoved, sp,
                     [sp](int, int) { apply_split_ratio_clamp(sp); });
}

} // namespace chrome

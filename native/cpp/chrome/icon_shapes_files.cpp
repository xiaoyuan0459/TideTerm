#include "cpp/chrome/icon_canvas.h"

namespace chrome::detail {

bool paint_file_icon(IconCanvas &c, const QString &name) {
    const auto n = [&](const char *value) { return name == QLatin1String(value); };
    if (n("folder") || n("new_dir")) {
        if (n("folder")) c.folder();
        else {
            c.poly({{14, 8}, {14, 6}, {8, 6}, {6, 4}, {1, 4}, {1, 13}, {7, 13}});
            c.plus(12, 12, 2);
        }
    } else if (n("file") || n("new_file")) {
        if (n("file")) {
            c.document();
            c.line(5, 8, 10, 8);
            c.line(5, 11, 9, 11);
        } else {
            c.poly({{13, 8}, {13, 5}, {9, 1}, {3, 1}, {3, 14}, {7, 14}});
            c.poly({{9, 1}, {9, 5}, {13, 5}});
            c.plus(12, 12, 2);
        }
    } else if (n("open")) {
        c.poly({{2, 11}, {2, 3}, {6, 3}, {8, 5}, {13, 5}});
        c.poly({{2, 13}, {5, 7}, {15, 7}, {12, 13}}, true);
    } else if (n("move")) {
        c.poly({{7, 5}, {5, 3}, {1, 3}, {1, 13}, {8, 13}});
        c.line(6, 8, 14, 8);
        c.poly({{11, 5}, {14, 8}, {11, 11}});
    } else if (n("download") || n("upload")) {
        c.poly({{2, 11}, {2, 14}, {14, 14}, {14, 11}});
        if (n("download")) {
            c.line(8, 2, 8, 10);
            c.poly({{4, 6}, {8, 10}, {12, 6}});
        } else {
            c.line(8, 10, 8, 2);
            c.poly({{4, 6}, {8, 2}, {12, 6}});
        }
    } else if (n("link")) {
        c.poly({{9, 6}, {11, 4}, {11, 2}, {9, 1}, {7, 2}, {4, 5}, {4, 7}, {6, 9}});
        c.poly({{7, 10}, {5, 12}, {5, 14}, {7, 15}, {9, 14}, {12, 11}, {12, 9}, {10, 7}});
        c.line(6, 10, 10, 6);
    } else if (n("path_term")) {
        c.terminal(1, 2, 11, 9);
        c.line(9, 14, 15, 14);
        c.poly({{13, 12}, {15, 14}, {13, 15}});
    } else if (n("secure")) {
        c.poly({{8, 1}, {14, 4}, {13, 10}, {8, 15}, {3, 10}, {2, 4}}, true);
        c.poly({{5, 8}, {7, 10}, {11, 6}});
    } else if (n("serial")) {
        c.poly({{1, 5}, {3, 11}, {13, 11}, {15, 5}}, true);
        for (int x : {5, 8, 11}) c.dot(x, 8);
        c.line(8, 11, 8, 14);
    } else if (n("network")) {
        c.rect(2, 1, 12, 7);
        c.line(8, 8, 8, 11);
        c.line(2, 11, 14, 11);
        c.rect(1, 11, 3, 3);
        c.rect(6, 11, 3, 3);
        c.rect(11, 11, 3, 3);
    } else {
        return false;
    }
    return true;
}

} // namespace chrome::detail

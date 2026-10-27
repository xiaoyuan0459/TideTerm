#pragma once
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/ui_theme.h"
#include <QStringListModel>
#include <QHash>
#include <vector>

// Qt holds only bounded display metadata from the current Rust registry snapshot.
class LiveSessionLabels final : public QStringListModel {
public:
    using QStringListModel::QStringListModel;
    QHash<uint32_t, QString> icons;
    QHash<uint32_t, uint8_t> states;
    std::vector<uint32_t> visible_ids;
    QVariant data(const QModelIndex &index, int role) const override {
        if (index.row() >= 0 && index.row() < int(visible_ids.size())) {
            const uint32_t id = visible_ids[size_t(index.row())];
            if (role == Qt::DecorationRole) return chrome::scalable_icon(
                icons.value(id, QStringLiteral("terminal")), 14);
            if (role == Qt::ForegroundRole && states.value(id) >= 4)
                return states.value(id) == 5 ? chrome::ui_colors().danger : chrome::ui_colors().text_muted;
        }
        return QStringListModel::data(index, role);
    }
};

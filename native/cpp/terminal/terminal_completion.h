#pragma once
#include "cpp/bridge_shim.h"
#include <QObject>
#include <QRect>
#include <QString>
#include <QTimer>
class PtyView;
class QTreeWidget;
class TerminalSuggestions final : public QObject {
public:
    explicit TerminalSuggestions(PtyView *view);
    void sync(bool repaint);
    void cancel();
protected:
    bool eventFilter(QObject *, QEvent *) override;
private:
    void refresh();
    void accept();
    void apply_style();
    void place_popup();
    QRect view_geometry_;
    PtyView *view_;
    QTreeWidget *popup_;
    QTimer delay_;
    TerminalCompletion snapshot_;
    QString style_;
    QString info_;
    bool editing_ = false;
    bool explicit_list_ = false;
    int accepting_key_ = 0;
};

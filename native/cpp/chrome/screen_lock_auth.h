#pragma once
#include <QObject>
#include <QString>
#include <QThread>
#include <functional>
#include <memory>

namespace chrome {
// The Rust verifier/file operation runs off the GUI thread. Qt owns only pending UI state.
// A closed receiver automatically disconnects completion; the worker still cleans itself up.
inline void run_lock_auth(QObject *receiver, std::function<QString()> operation,
                          std::function<void(const QString &)> completed) {
    auto result = std::make_shared<QString>();
    auto *worker = QThread::create([operation = std::move(operation), result] {
        *result = operation();
    });
    QObject::connect(worker, &QThread::finished, receiver,
        [result, completed = std::move(completed)] { completed(*result); }, Qt::QueuedConnection);
    QObject::connect(worker, &QThread::finished, worker, &QObject::deleteLater);
    worker->start();
}
}

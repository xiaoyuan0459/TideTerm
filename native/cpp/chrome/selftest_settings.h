#pragma once
#include <QSettings>
#include <QTemporaryDir>

// Each Qt self-test owns its layout store as well as its Rust data fixture.
// Standard QSettings calls exercise production persistence inside this directory.
namespace selftest_ui {
class SettingsScope {
public:
    SettingsScope() {
        if (!directory_.isValid()) qFatal("Cannot create Qt self-test settings directory");
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory_.path());
        QSettings::setPath(QSettings::IniFormat, QSettings::SystemScope, directory_.path());
    }
private:
    QTemporaryDir directory_;
};
}

/* === This file is part of the Scotia Calamares view module === */
#ifndef SCOTIA_VIEWSTEP_H
#define SCOTIA_VIEWSTEP_H

#include "viewpages/QmlViewStep.h"

class Config;

class PLUGINDLLEXPORT_PRO ScotiaViewStep : public Calamares::QmlViewStep
{
    Q_OBJECT

public:
    explicit ScotiaViewStep( QObject* parent = nullptr );
    ~ScotiaViewStep() override;

    QString prettyName() const override;
    bool isNextEnabled() const override;
    bool isBackEnabled() const override;

    void onLeave() override;
    void setConfigurationMap( const QVariantMap& configurationMap ) override;

protected:
    QObject* getConfig() override;

private:
    Config* m_config;
};

#endif  // SCOTIA_VIEWSTEP_H

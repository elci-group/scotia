/* === This file is part of the Scotia Calamares view module === */
#include "ScotiaViewStep.h"
#include "Config.h"

#include "GlobalStorage.h"
#include "JobQueue.h"
#include "utils/PluginFactory.h"
#include "utils/Variant.h"

ScotiaViewStep::ScotiaViewStep( QObject* parent )
    : Calamares::QmlViewStep( parent )
    , m_config( new Config( this ) )
{
}

ScotiaViewStep::~ScotiaViewStep() = default;

QString
ScotiaViewStep::prettyName() const
{
    return tr( "Scotia Setup" );
}

bool
ScotiaViewStep::isNextEnabled() const
{
    return true;
}

bool
ScotiaViewStep::isBackEnabled() const
{
    return true;
}

void
ScotiaViewStep::setConfigurationMap( const QVariantMap& configurationMap )
{
    m_config->setInstallScope( Calamares::getString( configurationMap, "scope", QStringLiteral( "user" ) ) );
    m_config->setAutostart( Calamares::getBool( configurationMap, "autostart", true ) );
    m_config->setInstallShims( Calamares::getBool( configurationMap, "installShims", true ) );

    Calamares::QmlViewStep::setConfigurationMap( configurationMap );
}

void
ScotiaViewStep::onLeave()
{
    auto* gs = Calamares::JobQueue::instance()->globalStorage();
    gs->insert( QStringLiteral( "scotiaScope" ), m_config->installScope() );
    gs->insert( QStringLiteral( "scotiaAutostart" ), m_config->autostart() );
    gs->insert( QStringLiteral( "scotiaInstallShims" ), m_config->installShims() );

    Calamares::QmlViewStep::onLeave();
}

QObject*
ScotiaViewStep::getConfig()
{
    return m_config;
}

CALAMARES_PLUGIN_FACTORY_DEFINITION( ScotiaViewStepFactory, registerPlugin< ScotiaViewStep >(); )

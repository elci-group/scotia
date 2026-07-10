/* === This file is part of the Scotia Calamares view module === */
#include "Config.h"

Config::Config( QObject* parent )
    : QObject( parent )
    , m_scope( QStringLiteral( "user" ) )
    , m_autostart( true )
    , m_installShims( true )
{
}

void
Config::setInstallScope( const QString& scope )
{
    if ( m_scope != scope )
    {
        m_scope = scope;
        emit installScopeChanged( m_scope );
    }
}

void
Config::setAutostart( bool autostart )
{
    if ( m_autostart != autostart )
    {
        m_autostart = autostart;
        emit autostartChanged( m_autostart );
    }
}

void
Config::setInstallShims( bool installShims )
{
    if ( m_installShims != installShims )
    {
        m_installShims = installShims;
        emit installShimsChanged( m_installShims );
    }
}

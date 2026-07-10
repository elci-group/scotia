/* === This file is part of the Scotia Calamares view module === */
#ifndef SCOTIA_CONFIG_H
#define SCOTIA_CONFIG_H

#include <QObject>

class PLUGINDLLEXPORT_PRO Config : public QObject
{
    Q_OBJECT
    Q_PROPERTY( QString installScope READ installScope WRITE setInstallScope NOTIFY installScopeChanged )
    Q_PROPERTY( bool autostart READ autostart WRITE setAutostart NOTIFY autostartChanged )
    Q_PROPERTY( bool installShims READ installShims WRITE setInstallShims NOTIFY installShimsChanged )

public:
    explicit Config( QObject* parent = nullptr );

    QString installScope() const { return m_scope; }
    bool autostart() const { return m_autostart; }
    bool installShims() const { return m_installShims; }

public slots:
    void setInstallScope( const QString& scope );
    void setAutostart( bool autostart );
    void setInstallShims( bool installShims );

signals:
    void installScopeChanged( const QString& scope );
    void autostartChanged( bool autostart );
    void installShimsChanged( bool installShims );

private:
    QString m_scope;
    bool m_autostart;
    bool m_installShims;
};

#endif  // SCOTIA_CONFIG_H

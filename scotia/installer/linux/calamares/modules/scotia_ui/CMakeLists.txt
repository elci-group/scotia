# CMake build for the Scotia QML Calamares view module.
# Copy this directory into the Calamares source tree under src/modules/
# and build Calamares as usual, or build it out-of-tree against an
# installed Calamares development package.

calamares_add_plugin(
    scotia_ui
    TYPE viewmodule
    EXPORT_MACRO PLUGINDLLEXPORT_PRO
    SOURCES
        ScotiaViewStep.cpp
        Config.cpp
    RESOURCES scotia.qrc
)

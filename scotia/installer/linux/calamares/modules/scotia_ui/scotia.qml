/* === Scotia Calamares setup page === */

import io.calamares.core 1.0
import io.calamares.ui 1.0

import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.3

Item {
    id: root
    width: parent.width
    height: parent.height

    Rectangle {
        anchors.fill: parent
        color: "#e8f4f8"

        ColumnLayout {
            anchors.centerIn: parent
            width: Math.min(parent.width - 80, 720)
            spacing: 24

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("Scotia Setup")
                font.pointSize: 22
                font.bold: true
                color: "#0b3d5c"
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("Choose how the Scotia daemon and agent shims are installed.")
                font.pointSize: 10
                color: "#2c5f7c"
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
            }

            Rectangle {
                Layout.fillWidth: true
                height: scopeColumn.height + 32
                color: "#ffffff"
                radius: 8
                border.color: "#b8d4e3"
                border.width: 1

                Column {
                    id: scopeColumn
                    anchors.centerIn: parent
                    width: parent.width - 32
                    spacing: 12

                    Text {
                        text: qsTr("Installation scope")
                        font.pointSize: 12
                        font.bold: true
                        color: "#0b3d5c"
                    }

                    Text {
                        width: parent.width
                        text: qsTr("A system-wide install writes service files for all users and requires root privileges. A per-user install only starts the daemon for the current user.")
                        font.pointSize: 9
                        color: "#2c5f7c"
                        wrapMode: Text.WordWrap
                    }

                    RadioButton {
                        text: qsTr("Current user only")
                        checked: config.installScope === "user"
                        onClicked: config.installScope = "user"
                    }

                    RadioButton {
                        text: qsTr("System-wide (requires root)")
                        checked: config.installScope === "system"
                        onClicked: config.installScope = "system"
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                height: optionsColumn.height + 32
                color: "#ffffff"
                radius: 8
                border.color: "#b8d4e3"
                border.width: 1

                Column {
                    id: optionsColumn
                    anchors.centerIn: parent
                    width: parent.width - 32
                    spacing: 12

                    Text {
                        text: qsTr("Startup and PATH options")
                        font.pointSize: 12
                        font.bold: true
                        color: "#0b3d5c"
                    }

                    Switch {
                        text: qsTr("Start the Scotia daemon automatically")
                        checked: config.autostart
                        onCheckedChanged: config.autostart = checked
                    }

                    Switch {
                        text: qsTr("Install PATH shims (kimi, claude, codex, ...)")
                        checked: config.installShims
                        onCheckedChanged: config.installShims = checked
                    }
                }
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("These choices will be applied when you click Next and the installer runs.")
                font.pointSize: 9
                color: "#5a7d91"
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
            }
        }
    }
}

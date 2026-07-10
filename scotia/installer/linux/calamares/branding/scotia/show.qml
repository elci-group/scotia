/* === Scotia Calamares slideshow === */

import QtQuick 2.15

Item {
    id: root

    property bool activatedInCalamares: false

    function onActivate() {
        activatedInCalamares = true;
    }

    function onLeave() {
        activatedInCalamares = false;
    }

    Rectangle {
        anchors.fill: parent
        color: "#0b3d5c"

        Column {
            anchors.centerIn: parent
            spacing: 16

            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Scotia"
                font.pointSize: 28
                font.bold: true
                color: "#e8f4f8"
            }

            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Semantic Decision Ledger for agentic systems"
                font.pointSize: 12
                color: "#8ecae6"
            }

            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Installing... light flurries at the harbour."
                font.pointSize: 10
                color: "#b8d4e3"
            }
        }
    }
}

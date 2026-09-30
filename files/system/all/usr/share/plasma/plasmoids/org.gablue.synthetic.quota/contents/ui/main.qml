/*
 * Widget panel Gablue — Quota Synthetic
 * Affiche les quotas Synthetic (tokens hebdo, requêtes 5 h) comme le plugin
 * TUI OpenCode (synthetic-quota/tui.tsx), avec le même code couleur.
 * La clé API est lue directement dans la db OpenCode (table credential) par
 * le helper Python contents/code/gablue-synthetic-quota-helper, exécuté via
 * le data engine "executable" (plasma5support).
 */

import QtQuick
import QtQuick.Layouts
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components 3.0 as PlasmaComponents
import org.kde.plasma.plasmoid 2.0
import org.kde.plasma.plasma5support 2.0 as P5Support
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root

    // =========================================================================
    // Configuration (contents/config/main.xml)
    // =========================================================================

    readonly property int refreshInterval: Plasmoid.configuration.refreshInterval
    readonly property int decimals: Plasmoid.configuration.decimals
    readonly property bool showReq: Plasmoid.configuration.showReq

    // =========================================================================
    // État
    // =========================================================================

    // Dernier JSON du helper (champs : weekly, fiveHour, subscription, …)
    property var quotas: null
    // "no-key", "HTTP 401", nom d'exception, "" si tout va bien
    property string errorText: ""

    readonly property bool noKey: errorText === "no-key"
    readonly property bool stale: quotas !== null && quotas.stale === true
    readonly property bool loading: quotas === null && !noKey

    readonly property var weekly: quotas !== null ? quotas.weekly : null
    readonly property var fiveHour: quotas !== null ? quotas.fiveHour : null
    readonly property var subscription: quotas !== null ? quotas.subscription : null

    // Quota principal : weekly, sinon fiveHour, sinon subscription (fallback TUI)
    readonly property var primaryPct: {
        if (weekly !== null && weekly !== undefined) {
            return weekly
        }
        if (fiveHour !== null && fiveHour !== undefined) {
            return fiveHour
        }
        return subscription
    }

    readonly property var reqPct:
        fiveHour !== null && fiveHour !== undefined ? fiveHour : null

    readonly property bool isVerticalPanel:
        Plasmoid.location === PlasmaCore.Types.LeftEdge
        || Plasmoid.location === PlasmaCore.Types.RightEdge

    readonly property string creditsText: {
        if (quotas === null) {
            return ""
        }
        const rem = quotas.weeklyRemaining
        const max = quotas.weeklyMax
        return (rem && max) ? (rem + " / " + max) : ""
    }

    // =========================================================================
    // Helper Python (chemin absolu dérivé de l'URL de ce fichier)
    // =========================================================================

    readonly property string helperCmd: {
        let url = Qt.resolvedUrl("../code/gablue-synthetic-quota-helper").toString()
        if (url.startsWith("file://")) {
            url = url.substring(7)
        }
        return "/usr/bin/python3 " + url
    }

    // =========================================================================
    // Fonctions
    // =========================================================================

    // Pourcentage formaté (2 décimales comme le TUI) — "?" si absent
    function fmt(pct) {
        if (pct === null || pct === undefined) {
            return "?"
        }
        return Number(pct).toFixed(decimals) + "%"
    }

    // Code couleur identique au plugin TUI (adapté au thème Plasma)
    function stateColor(pct) {
        if (pct === null || pct === undefined) {
            return Kirigami.Theme.disabledTextColor
        }
        if (pct <= 20) {
            return Kirigami.Theme.negativeTextColor
        }
        if (pct <= 50) {
            return Kirigami.Theme.neutralTextColor
        }
        return Kirigami.Theme.positiveTextColor
    }

    function formatIso(iso) {
        if (!iso) {
            return "—"
        }
        const d = new Date(iso)
        return isNaN(d.getTime()) ? iso : Qt.formatDateTime(d)
    }

    function applyResult(stdout) {
        let data = null
        try {
            data = JSON.parse(stdout)
        } catch (e) {
            errorText = "réponse invalide"
            return
        }
        quotas = data
        errorText = data.error ? String(data.error) : ""
    }

    // Texte du panel — format TOK xx% · REQ yy% (identique au plugin TUI)
    readonly property string displayText: {
        if (noKey) {
            return "TOK ?"
        }
        if (loading) {
            return "…"
        }
        const w = weekly
        const f = fiveHour
        const hasW = w !== null && w !== undefined
        const hasF = f !== null && f !== undefined
        if (!hasW && !hasF) {
            return "TOK " + fmt(subscription)
        }
        let out = "TOK " + fmt(w)
        if (showReq && hasF) {
            out += " · REQ " + fmt(f)
        }
        return out
    }

    function tooltipSubText() {
        if (noKey) {
            return "Clé Synthetic introuvable dans la db OpenCode\n"
                + "(~/.local/share/opencode/opencode.db, table credential)"
        }
        if (quotas === null) {
            return "Chargement…"
        }
        const lines = []
        if (weekly !== null && weekly !== undefined) {
            lines.push("Tokens (7 j) : " + fmt(weekly))
        }
        if (fiveHour !== null && fiveHour !== undefined) {
            lines.push("Requêtes (5 h) : " + fmt(fiveHour))
        }
        if (lines.length === 0 && subscription !== null && subscription !== undefined) {
            lines.push("Abonnement : " + fmt(subscription))
        }
        if (creditsText !== "") {
            lines.push("Crédits : " + creditsText)
        }
        if (quotas.limited === true) {
            lines.push("Limite 5 h atteinte")
        }
        if (errorText !== "") {
            lines.push("⚠ " + (stale ? "données en cache — " : "") + errorText)
        }
        return lines.join("\n")
    }

    // =========================================================================
    // Data engine "executable" : lance le helper, relance à chaque intervalle
    // =========================================================================

    P5Support.DataSource {
        id: execSource

        engine: "executable"
        interval: root.refreshInterval * 1000
        // Signature Plasma 6 : newData(sourceName, data)
        onNewData: (source, data) => root.applyResult(data["stdout"])
        Component.onCompleted: connectSource(root.helperCmd)
    }

    // =========================================================================
    // Représentations
    // =========================================================================

    compactRepresentation: CompactRep {}
    fullRepresentation: FullRep {}

    // -------------------------------------------------------------------------
    // Représentation compacte : texte coloré dans le panel + tooltip
    // -------------------------------------------------------------------------
    component CompactRep: Item {
        id: compact

        Layout.minimumWidth: quotaText.implicitWidth
        Layout.minimumHeight: quotaText.implicitHeight
        Layout.preferredWidth: quotaText.implicitWidth
        Layout.preferredHeight: quotaText.implicitHeight

        Text {
            id: quotaText

            anchors.centerIn: parent
            // Panel vertical : pourcentage seul, sinon format complet
            text: root.isVerticalPanel ? root.fmt(root.primaryPct) : root.displayText
            color: root.stateColor(root.primaryPct)
            font.family: Kirigami.Theme.defaultFont.family
            // S'adapte à l'épaisseur du panel, borné à la police par défaut
            font.pixelSize: compact.height > 0
                ? Math.min(compact.height * 0.62, Kirigami.Theme.defaultFont.pixelSize)
                : Kirigami.Theme.defaultFont.pixelSize
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }

        PlasmaCore.ToolTipArea {
            anchors.fill: parent
            mainText: "Synthetic"
            subText: root.tooltipSubText()
            icon: "speedometer"
        }
    }

    // -------------------------------------------------------------------------
    // Représentation étendue (popup au clic) : barres + détails
    // -------------------------------------------------------------------------
    component FullRep: ColumnLayout {
        id: full

        readonly property int pad: Kirigami.Units.gridUnit

        Layout.minimumWidth: Kirigami.Units.gridUnit * 22
        Layout.preferredWidth: Kirigami.Units.gridUnit * 22
        Layout.minimumHeight: full.implicitHeight + pad
        Layout.preferredHeight: full.implicitHeight + pad
        Layout.maximumWidth: Kirigami.Units.gridUnit * 22

        spacing: Kirigami.Units.smallSpacing

        Kirigami.Heading {
            level: 4
            text: "Quota Synthetic"
        }

        // ---- Tokens hebdo ----
        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true

                PlasmaComponents.Label {
                    Layout.fillWidth: true
                    text: "Tokens (7 jours)"
                }
                PlasmaComponents.Label {
                    text: root.fmt(root.weekly)
                    color: root.stateColor(root.weekly)
                }
            }
            PlasmaComponents.ProgressBar {
                Layout.fillWidth: true
                from: 0
                to: 100
                visible: root.weekly !== null && root.weekly !== undefined
                value: visible ? Math.max(0, Math.min(100, root.weekly)) : 0
            }
        }

        // ---- Requêtes 5 h ----
        ColumnLayout {
            Layout.fillWidth: true
            visible: root.fiveHour !== null && root.fiveHour !== undefined
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true

                PlasmaComponents.Label {
                    Layout.fillWidth: true
                    text: "Requêtes (5 heures)"
                }
                PlasmaComponents.Label {
                    text: root.fmt(root.fiveHour)
                    color: root.stateColor(root.fiveHour)
                }
            }
            PlasmaComponents.ProgressBar {
                Layout.fillWidth: true
                from: 0
                to: 100
                value: visible ? Math.max(0, Math.min(100, root.fiveHour)) : 0
            }
        }

        // ---- Fallback abonnement (si ni weekly ni fiveHour) ----
        ColumnLayout {
            Layout.fillWidth: true
            visible: (root.weekly === null || root.weekly === undefined)
                && (root.fiveHour === null || root.fiveHour === undefined)
                && root.subscription !== null && root.subscription !== undefined
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true

                PlasmaComponents.Label {
                    Layout.fillWidth: true
                    text: "Abonnement"
                }
                PlasmaComponents.Label {
                    text: root.fmt(root.subscription)
                    color: root.stateColor(root.subscription)
                }
            }
            PlasmaComponents.ProgressBar {
                Layout.fillWidth: true
                from: 0
                to: 100
                value: visible ? Math.max(0, Math.min(100, root.subscription)) : 0
            }
        }

        // ---- Limite 5 h atteinte ----
        PlasmaComponents.Label {
            visible: root.quotas !== null && root.quotas.limited === true
            text: "⚠ Limite 5 heures atteinte"
            color: Kirigami.Theme.negativeTextColor
        }

        // ---- Détails ----
        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            PlasmaComponents.Label {
                visible: root.creditsText !== ""
                text: "Crédits : " + root.creditsText
                opacity: 0.8
            }
            PlasmaComponents.Label {
                visible: root.quotas !== null && root.quotas.nextRegenAt
                text: "Régénération : " + root.formatIso(root.quotas ? root.quotas.nextRegenAt : null)
                opacity: 0.8
            }
            PlasmaComponents.Label {
                visible: root.quotas !== null && root.quotas.nextTickAt
                text: "Reset 5 h : " + root.formatIso(root.quotas ? root.quotas.nextTickAt : null)
                opacity: 0.8
            }
            PlasmaComponents.Label {
                visible: root.quotas !== null && root.quotas.renewsAt
                text: "Renouvellement : " + root.formatIso(root.quotas ? root.quotas.renewsAt : null)
                opacity: 0.8
            }
            PlasmaComponents.Label {
                visible: root.quotas !== null && root.quotas.fetchedAt > 0
                text: "MAJ : " + Qt.formatTime(new Date((root.quotas ? root.quotas.fetchedAt : 0) * 1000))
                opacity: 0.6
                font.italic: true
            }
        }

        // ---- Erreur / stale ----
        PlasmaComponents.Label {
            Layout.fillWidth: true
            visible: root.errorText !== ""
            text: root.stale
                ? "⚠ Données en cache — " + root.errorText
                : "⚠ " + root.errorText
            color: Kirigami.Theme.negativeTextColor
            font.italic: true
            elide: Text.ElideMiddle
            maximumLineCount: 2
            wrapMode: Text.Wrap
        }
    }
}

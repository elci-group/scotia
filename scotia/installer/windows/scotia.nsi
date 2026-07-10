; Scotia Windows installer
; Build: copy release binaries to bin/ next to this script, then run:
;   makensis scotia.nsi

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"

Name "Scotia"
OutFile "Scotia-Setup.exe"
InstallDir "$LOCALAPPDATA\Scotia"
RequestExecutionLevel user

; --- Install scope ----------------------------------------------------------
; Per-user is the supported and default install: the Scotia daemon is a
; per-user process (a system-wide daemon is explicitly unsupported, matching
; the Linux installer's refusal to run as root), so this installer never
; requests elevation by default and writes under %LOCALAPPDATA% and HKCU.
;
; The "all users" radio on the scope page exists ONLY for managed/enterprise
; deployments that want the binaries under %PROGRAMFILES64% and a HKLM PATH
; entry. Selecting it requires running this EXE elevated (right-click ->
; "Run as administrator") or building a separate admin variant with
; `RequestExecutionLevel admin`. Without elevation the HKLM / PROGRAMFILES64
; writes in that branch will fail; this is intentional, not a silent fallback.

; Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE"
Page custom ScopePageCreate ScopePageLeave
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

; Installer options
Var ScopeRadioUser
Var ScopeRadioSystem
Var AutostartCheck
Var ShimsCheck
Var InstallScope

Function ScopePageCreate
    nsDialogs::Create 1018
    Pop $0

    ${NSD_CreateLabel} 0 0 100% 12u "Choose how Scotia should be installed:"
    Pop $0

    ${NSD_CreateRadioButton} 0 20u 100% 12u "Install for the current user only"
    Pop $ScopeRadioUser
    ${NSD_CreateRadioButton} 0 36u 100% 12u "Install for all users (requires Administrator)"
    Pop $ScopeRadioSystem

    ${If} $InstallScope == "system"
        ${NSD_Check} $ScopeRadioSystem
    ${Else}
        ${NSD_Check} $ScopeRadioUser
    ${EndIf}

    ${NSD_CreateCheckbox} 0 64u 100% 12u "Start scotiad automatically"
    Pop $AutostartCheck
    ${NSD_Check} $AutostartCheck

    ${NSD_CreateCheckbox} 0 80u 100% 12u "Add Scotia shims to PATH"
    Pop $ShimsCheck
    ${NSD_Check} $ShimsCheck

    nsDialogs::Show
FunctionEnd

Function ScopePageLeave
    ${NSD_GetState} $ScopeRadioSystem $0
    ${If} $0 == "1"
        StrCpy $InstallScope "system"
        StrCpy $INSTDIR "$PROGRAMFILES64\Scotia"
    ${Else}
        StrCpy $InstallScope "user"
        StrCpy $INSTDIR "$LOCALAPPDATA\Scotia"
    ${EndIf}
FunctionEnd

Section "Scotia" SecScotia
    SetOutPath "$INSTDIR\bin"
    File /r "bin\*.*"

    ; Add the bin directory to PATH so scotia.exe and shims are found.
    ${NSD_GetState} $ShimsCheck $0
    ${If} $0 == "1"
        ${If} $InstallScope == "system"
            ; Requires elevation (see scope note above): writes to HKLM.
            EnVar::SetHKLM
        ${Else}
            EnVar::SetHKCU
        ${EndIf}
        EnVar::AddValue "PATH" "$INSTDIR\bin"
    ${EndIf}

    ; Build the CLI arguments and run the Rust installer.
    ${NSD_GetState} $AutostartCheck $0
    ${If} $0 == "1"
        StrCpy $1 "--autostart"
    ${Else}
        StrCpy $1 ""
    ${EndIf}

    ${NSD_GetState} $ShimsCheck $0
    ${If} $0 == "1"
        StrCpy $2 "--install-shims"
    ${Else}
        StrCpy $2 ""
    ${EndIf}

    nsExec::ExecToLog '\"$INSTDIR\bin\scotia.exe\" installer apply --scope $InstallScope $1 $2 --bin-dir "$INSTDIR\bin"'

    WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
    nsExec::ExecToLog '\"$INSTDIR\bin\scotia.exe\" daemon stop'
    nsExec::ExecToLog 'sc.exe stop Scotia'
    nsExec::ExecToLog 'sc.exe delete Scotia'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ScotiaDaemon"

    EnVar::SetHKCU
    EnVar::DeleteValue "PATH" "$INSTDIR\bin"
    EnVar::SetHKLM
    EnVar::DeleteValue "PATH" "$INSTDIR\bin"

    RMDir /r "$INSTDIR"
SectionEnd

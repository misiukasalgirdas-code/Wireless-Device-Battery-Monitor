//! User interface language: English or Lithuanian.
//!
//! The choice is saved as "Language" ("en" / "lt") under the taskbar edition's settings key
//! (right-click menu > Language). Without a saved choice the Windows display language decides:
//! Lithuanian Windows -> Lithuanian, anything else -> English.
//!
//! Texts are written in English in the code and looked up with [`tr`] / [`trf`]; the Lithuanian
//! table below maps each English text to its translation. A text missing from the table is shown
//! in English. Placeholders in [`trf`] texts are `{0}`, `{1}`, ... in both languages.
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Lt,
}

impl Lang {
    /// Every language offered in the menu.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Lt];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Lt => "lt",
        }
    }

    /// The language's name in that language (menu entries).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Lt => "Lietuvių",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Lang::En),
            "lt" => Some(Lang::Lt),
            _ => None,
        }
    }
}

const VALUE: &str = "Language";
/// 0 = not decided yet, 1 = English, 2 = Lithuanian.
static CURRENT: AtomicU8 = AtomicU8::new(0);

fn settings_key() -> &'static str {
    crate::install::TASKBAR.settings_key
}

/// The language chosen in the menu, if any.
pub fn saved() -> Option<Lang> {
    crate::util::reg_get_string(settings_key(), VALUE).and_then(|v| Lang::from_code(&v))
}

/// Lithuanian when the Windows display language is Lithuanian, else English.
pub fn system() -> Lang {
    const LANG_LITHUANIAN: u16 = 0x27;
    let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    if id & 0x3FF == LANG_LITHUANIAN { Lang::Lt } else { Lang::En }
}

/// The language used for the user interface (decided once, then cached).
pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        2 => Lang::Lt,
        _ => {
            // Unit tests compare English texts, whatever the language of the PC running them.
            let l = if cfg!(test) { Lang::En } else { saved().unwrap_or_else(system) };
            set_current(l);
            l
        }
    }
}

fn set_current(l: Lang) {
    CURRENT.store(if l == Lang::En { 1 } else { 2 }, Ordering::Relaxed);
}

/// Saves `l` as the user's choice and switches to it.
pub fn choose(l: Lang) {
    crate::util::reg_set_string(settings_key(), VALUE, l.code());
    set_current(l);
}

/// `en` in the current language.
pub fn tr(en: &'static str) -> &'static str {
    match current() {
        Lang::En => en,
        Lang::Lt => lookup(en).unwrap_or(en),
    }
}

/// Like [`tr`] for a text that is not a literal (e.g. a coarse level a driver reported).
pub fn tr_any(en: &str) -> &str {
    match current() {
        Lang::En => en,
        Lang::Lt => lookup(en).unwrap_or(en),
    }
}

/// `en` in the current language with `{0}`, `{1}`, ... replaced by `args`.
pub fn trf(en: &'static str, args: &[&dyn Display]) -> String {
    fill(tr(en), args)
}

fn fill(text: &str, args: &[&dyn Display]) -> String {
    let mut s = text.to_string();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}

fn lookup(en: &str) -> Option<&'static str> {
    LT.iter().find(|(e, _)| *e == en).map(|(_, t)| *t)
}

/// English -> Lithuanian.
const LT: &[(&str, &str)] = &[
    // ---- device states (device.rs) ----
    ("disconnected", "atjungta"),
    (" (asleep)", " (miega)"),
    (" (full)", " (įkrauta)"),
    (" (charging)", " (kraunasi)"),
    ("fully charged", "pilnai įkrauta"),
    ("charging", "kraunasi"),
    ("asleep, last known level", "miega, paskutinis žinomas lygis"),
    ("on battery", "veikia baterija"),
    // Coarse levels some devices report instead of a percentage (drivers).
    ("Full", "Pilna"),
    ("Good", "Gera"),
    ("Medium", "Vidutinė"),
    ("Low", "Žema"),
    ("Critical", "Kritinė"),
    ("Empty", "Tuščia"),
    ("Charging", "Kraunasi"),
    // Device kinds (DeviceKind::label) and the request window's types (report.rs TYPES).
    ("Mouse", "Pelė"),
    ("Keyboard", "Klaviatūra"),
    ("Headset", "Ausinės"),
    ("Controller", "Žaidimų pultelis"),
    ("Phone", "Telefonas"),
    ("Microphone", "Mikrofonas"),
    ("mouse", "pelė"),
    ("keyboard", "klaviatūra"),
    ("headset", "ausinės"),
    ("controller", "žaidimų pultelis"),
    ("other", "kita"),
    // ---- warning levels (settings.rs) ----
    ("Red warning at or below:", "Raudonas įspėjimas, kai liko ne daugiau:"),
    ("  (default)", "  (numatyta)"),
    // ---- taskbar: tooltip, menu ----
    ("BatteryHub: no devices found", "BatteryHub: įrenginių nerasta"),
    ("Refresh devices", "Atnaujinti įrenginius"),
    ("Refresh battery levels", "Atnaujinti baterijų lygius"),
    ("Refresh battery every", "Baterijos lygį tikrinti kas"),
    ("1 minute", "1 minutę"),
    ("5 minutes", "5 minutes"),
    ("10 minutes", "10 minučių"),
    ("30 minutes", "30 minučių"),
    ("1 hour", "1 valandą"),
    ("Refresh when a device connects or disconnects", "Atnaujinti, kai įrenginys prisijungia ar atsijungia"),
    (
        "Background behind taskbar text (for translucent taskbars)",
        "Fonas po tekstu užduočių juostoje (permatomoms užduočių juostoms)",
    ),
    ("Next to the notification area", "Šalia pranešimų srities"),
    ("{0} px to the left", "{0} px į kairę"),
    ("Custom ({0} px, TaskbarOffset in the registry)", "Kita ({0} px, TaskbarOffset registre)"),
    ("Position on the taskbar", "Padėtis užduočių juostoje"),
    ("Centred", "Per vidurį"),
    ("{0} px up", "{0} px aukštyn"),
    ("{0} px down", "{0} px žemyn"),
    ("Custom ({0} px, TaskbarOffsetY in the registry)", "Kita ({0} px, TaskbarOffsetY registre)"),
    ("Height on the taskbar", "Aukštis užduočių juostoje"),
    ("All monitors", "Visuose monitoriuose"),
    ("Main monitor only", "Tik pagrindiniame monitoriuje"),
    ("{0} only", "Tik: {0}"),
    ("Chosen monitor (not connected, shown on main)", "Pasirinktas monitorius (neprijungtas, rodoma pagrindiniame)"),
    ("Show widget on", "Kur rodyti valdiklį"),
    ("Other monitor", "Kitas monitorius"),
    ("Display {0}", "Ekranas {0}"),
    ("left", "kairėje"),
    ("right", "dešinėje"),
    ("above", "viršuje"),
    ("below", "apačioje"),
    ("Automatically start with Windows", "Paleisti kartu su Windows"),
    ("Automatically check for updates", "Automatiškai tikrinti atnaujinimus"),
    ("Install updates silently", "Atnaujinimus diegti tyliai"),
    ("Language", "Kalba"),
    ("Request support for a device...", "Prašyti įrenginio palaikymo..."),
    ("Checking for updates...", "Tikrinami atnaujinimai..."),
    ("Check for updates...", "Tikrinti atnaujinimus..."),
    ("Exit", "Išeiti"),
    // ---- taskbar: update messages ----
    ("BatteryHub - Check for updates", "BatteryHub – atnaujinimai"),
    (
        "No new updates.\n\nYou have the latest version ({0}).\nLatest release on GitHub: {1}.",
        "Naujų atnaujinimų nėra.\n\nNaudojate naujausią versiją ({0}).\nNaujausia versija GitHub: {1}.",
    ),
    (
        "Could not check for updates:\n{0}\n\nNew versions are also on GitHub:\nhttps://github.com/{1}/releases",
        "Nepavyko patikrinti atnaujinimų:\n{0}\n\nNaujas versijas rasite ir GitHub:\nhttps://github.com/{1}/releases",
    ),
    (
        "A new update is available:\n\n        {0}  ->  {1}\n\nDo you want to update now?\n\nThe setup is downloaded from GitHub, checked against the release's SHA-256 checksum and started; it keeps your settings and autostart.",
        "Yra naujas atnaujinimas:\n\n        {0}  ->  {1}\n\nAr atnaujinti dabar?\n\nDiegimo programa atsiunčiama iš GitHub, patikrinama pagal versijos SHA-256 kontrolinę sumą ir paleidžiama; jūsų nustatymai ir automatinis paleidimas išlieka.",
    ),
    ("Could not start the setup:\n{0}\n\n{1}", "Nepavyko paleisti diegimo programos:\n{0}\n\n{1}"),
    ("The update could not be downloaded:\n{0}", "Nepavyko atsisiųsti atnaujinimo:\n{0}"),
    (
        "Automatic update check is on.\n\nBatteryHub looks for a new version only when it starts (at sign-in or when you start it), not while it runs. When there is one, it asks you whether to update (or, with \"Install updates silently\", installs it in the background).",
        "Automatinis atnaujinimų tikrinimas įjungtas.\n\nBatteryHub naujos versijos ieško tik paleidimo metu (prisijungus prie Windows arba paleidus rankiniu būdu), o ne veikdama. Radusi naują versiją, paklaus, ar atnaujinti (arba, jei įjungta „Atnaujinimus diegti tyliai“, įdiegs ją fone).",
    ),
    (
        "Silent updates are on.\n\nWhen BatteryHub starts and finds a new version, it downloads it from GitHub, checks its SHA-256 checksum and installs it in the background, without asking and without showing anything. Your settings and autostart are kept.",
        "Tylieji atnaujinimai įjungti.\n\nKai BatteryHub paleidimo metu randa naują versiją, ji atsisiunčia ją iš GitHub, patikrina SHA-256 kontrolinę sumą ir įdiegia fone – nieko neklausdama ir nieko nerodydama. Jūsų nustatymai ir automatinis paleidimas išlieka.",
    ),
    (
        "Update finished.\n\nBatteryHub was updated:  {0}  ->  {1}\n\nYour settings and autostart were kept.",
        "Atnaujinimas baigtas.\n\nBatteryHub atnaujinta:  {0}  ->  {1}\n\nJūsų nustatymai ir automatinis paleidimas išliko.",
    ),
    // ---- "Request support for a device" window ----
    ("BatteryHub: request support for a device", "BatteryHub: prašymas palaikyti įrenginį"),
    (
        "Is one of your devices not shown? Pick it below. Send opens a new issue on the project's GitHub page, filled in with what is shown here; you check it and submit it there (a GitHub account is needed).",
        "Jūsų įrenginys nerodomas? Pasirinkite jį žemiau. „Siųsti“ atidarys naują pranešimą projekto GitHub puslapyje su čia rodoma informacija; ten jį peržiūrėsite ir pateiksite (reikia GitHub paskyros).",
    ),
    ("Device", "Įrenginys"),
    ("Scanning devices...", "Ieškoma įrenginių..."),
    ("Device name (as it's sold)", "Įrenginio pavadinimas (kaip parduodamas)"),
    ("Type", "Tipas"),
    ("Your name (optional, shown in the GitHub issue)", "Jūsų vardas (neprivaloma, bus matomas GitHub pranešime)"),
    ("Anything else we should know? (optional)", "Ką dar turėtume žinoti? (neprivaloma)"),
    ("This is exactly what the GitHub issue will contain:", "Būtent tai bus GitHub pranešime (anglų kalba):"),
    ("Scanning your devices...", "Ieškoma jūsų įrenginių..."),
    ("Copy full report", "Kopijuoti visą ataskaitą"),
    ("Send", "Siųsti"),
    ("Close", "Uždaryti"),
    ("(type the device name)", "(įveskite įrenginio pavadinimą)"),
    ("My device is not in this list", "Mano įrenginio šiame sąraše nėra"),
    (
        "Pick the device that isn't shown. Devices with a vendor-specific interface or audio are listed first.",
        "Pasirinkite nerodomą įrenginį. Įrenginiai su gamintojo sąsaja ar garso funkcija rodomi pirmiausia.",
    ),
    (
        "GitHub opened in your browser with the request filled in. Check it and click Create to submit it. Thank you!",
        "Naršyklėje atsidarė GitHub su užpildytu prašymu. Peržiūrėkite jį ir spauskite „Create“, kad pateiktumėte. Ačiū!",
    ),
    (
        "Couldn't open the browser. The request is copied: paste it into a new issue at github.com/{0}/issues.",
        "Nepavyko atidaryti naršyklės. Prašymas nukopijuotas: įklijuokite jį į naują pranešimą adresu github.com/{0}/issues.",
    ),
    (
        "Couldn't open the browser. Please open a new issue at github.com/{0}/issues.",
        "Nepavyko atidaryti naršyklės. Sukurkite naują pranešimą adresu github.com/{0}/issues.",
    ),
    (
        "The full report (with every device BatteryHub sees) is copied. Paste it into the GitHub issue if we ask for more details.",
        "Visa ataskaita (su visais BatteryHub matomais įrenginiais) nukopijuota. Įklijuokite ją į GitHub pranešimą, jei paprašysime daugiau informacijos.",
    ),
    ("Couldn't copy to the clipboard. Please try again.", "Nepavyko nukopijuoti į iškarpinę. Bandykite dar kartą."),
    // ---- setup: console ----
    ("[Y/n]", "[T/n]"),
    ("[y/N]", "[t/N]"),
    ("     Please answer y or n.", "     Atsakykite t (taip) arba n (ne)."),
    ("\nPress Enter to close this window...", "\nPaspauskite Enter, kad uždarytumėte šį langą..."),
    ("\n  Choose an option [{0}]: ", "\n  Pasirinkite [{0}]: "),
    ("Please type one of the numbers shown.", "Įveskite vieną iš parodytų skaičių."),
    ("{0}  (missing)", "{0}  (nėra)"),
    ("{0}  ({1}, modified {2})", "{0}  ({1}, pakeista {2})"),
    ("on (HKCU\\{0})", "įjungta (HKCU\\{0})"),
    ("off", "išjungta"),
    ("yes", "taip"),
    ("no", "ne"),
    ("{0} bytes", "{0} baitų"),
    ("Scanning this PC for existing BatteryHub installations", "Ieškoma jau įdiegtų BatteryHub šiame kompiuteryje"),
    ("Reading HKCU\\{0}", "Skaitoma HKCU\\{0}"),
    ("(not present)", "(nėra)"),
    ("Checking install folder {0}", "Tikrinamas diegimo aplankas {0}"),
    ("(folder does not exist)", "(aplanko nėra)"),
    ("Checking desktop shortcut {0}", "Tikrinama darbalaukio nuoroda {0}"),
    ("found", "rasta"),
    ("not found", "nerasta"),
    ("Checking Start menu shortcut {0}", "Tikrinama meniu Pradžia nuoroda {0}"),
    ("Checking autostart HKCU\\{0}", "Tikrinamas automatinis paleidimas HKCU\\{0}"),
    ("not set", "nenustatyta"),
    ("Checking whether {0} is running", "Tikrinama, ar {0} veikia"),
    ("running", "veikia"),
    ("not running", "neveikia"),
    ("Checking for the original BatteryHub (C++)", "Ieškoma originalios BatteryHub (C++)"),
    ("found: {0}", "rasta: {0}"),
    ("CURRENT STATE", "DABARTINĖ BŪSENA"),
    ("INSTALLED  v{0}", "ĮDIEGTA  v{0}"),
    ("INSTALLED  (unknown version)", "ĮDIEGTA  (nežinoma versija)"),
    ("NOT INSTALLED", "NEĮDIEGTA"),
    ("This setup contains", "Šioje diegimo programoje"),
    ("Install folder", "Diegimo aplankas"),
    ("Program file", "Programos failas"),
    ("Desktop shortcut", "Darbalaukio nuoroda"),
    ("Start menu shortcut", "Meniu Pradžia nuoroda"),
    ("(none)", "(nėra)"),
    ("Settings", "Nustatymai"),
    ("Autostart", "Automatinis paleidimas"),
    ("Running now", "Dabar veikia"),
    ("Original BatteryHub", "Originali BatteryHub"),
    ("{0} (separate - not affected)", "{0} (atskira – neliečiama)"),
    ("not installed", "neįdiegta"),
    ("Writing {0} ({1})", "Rašoma {0} ({1})"),
    ("file is in use ({0}), retrying ({1}/20)...", "failas naudojamas ({0}), bandoma iš naujo ({1}/20)..."),
    ("Cannot write {0}", "Nepavyko įrašyti {0}"),
    ("Write error: {0}", "Rašymo klaida: {0}"),
    ("Verified (CRC-32 0x{0})", "Patikrinta (CRC-32 0x{0})"),
    ("Verification failed (CRC-32 0x{0})", "Patikrinti nepavyko (CRC-32 0x{0})"),
    ("Saving install information to HKCU\\{0}", "Diegimo informacija įrašoma į HKCU\\{0}"),
    ("Could not save install information.", "Nepavyko įrašyti diegimo informacijos."),
    ("running from {0}", "paleista iš {0}"),
    ("Copied the setup to {0} ({1})", "Diegimo programa nukopijuota į {0} ({1})"),
    (
        "Could not copy the setup to {0}: {1} (no entry in Settings > Apps)",
        "Nepavyko nukopijuoti diegimo programos į {0}: {1} (įrašo Parametrai > Programos nebus)",
    ),
    (
        "Cannot find this setup's own file: {0} (no entry in Settings > Apps)",
        "Nerastas šios diegimo programos failas: {0} (įrašo Parametrai > Programos nebus)",
    ),
    ("Registering in Settings > Apps: HKCU\\{0}", "Registruojama Parametrai > Programos: HKCU\\{0}"),
    ("Could not create the Settings > Apps entry.", "Nepavyko sukurti įrašo Parametrai > Programos."),
    ("Deleted HKCU\\{0}", "Ištrinta HKCU\\{0}"),
    (
        "{0} will be deleted a few seconds after this window closes",
        "{0} bus ištrintas praėjus kelioms sekundėms po šio lango uždarymo",
    ),
    ("Could not schedule deleting {0}: {1}", "Nepavyko suplanuoti {0} ištrynimo: {1}"),
    ("created folder {0}", "sukurtas aplankas {0}"),
    ("Desktop", "darbalaukio"),
    ("Start menu", "meniu Pradžia"),
    ("Creating {0} shortcut", "Kuriama {0} nuoroda"),
    ("Could not create {0}: {1}", "Nepavyko sukurti {0}: {1}"),
    ("Cannot open the autostart registry key.", "Nepavyko atidaryti automatinio paleidimo registro rakto."),
    ("Autostart on: HKCU\\{0} = {1}", "Automatinis paleidimas įjungtas: HKCU\\{0} = {1}"),
    ("cleared an old Task Manager 'disabled' state", "išvalyta sena Užduočių tvarkytuvės būsena „išjungta“"),
    ("Autostart off: removed the existing autostart entry.", "Automatinis paleidimas išjungtas: esamas įrašas pašalintas."),
    ("Autostart off.", "Automatinis paleidimas išjungtas."),
    (
        "Scanning for supported wireless devices (this can take a few seconds)",
        "Ieškoma palaikomų belaidžių įrenginių (gali užtrukti kelias sekundes)",
    ),
    ("{0} HID collections on {1} USB / Bluetooth devices", "{0} HID kolekcijos (-ų) {1} USB / Bluetooth įrenginiuose"),
    ("(no product name)", "(be produkto pavadinimo)"),
    (
        "No supported device with a battery is connected right now. BatteryHub will pick them up later.",
        "Šiuo metu neprijungtas joks palaikomas įrenginys su baterija. BatteryHub juos aptiks vėliau.",
    ),
    ("via", "per"),
    ("{0} started (process id {1}).", "{0} paleista (proceso ID {1})."),
    ("Could not start {0}: {1}", "Nepavyko paleisti {0}: {1}"),
    ("  (nothing)", "  (nieko)"),
    ("[1/6] Updating {0} v{1} - stopping a running copy", "[1/6] Atnaujinama {0} v{1} – stabdoma veikianti kopija"),
    ("[1/6] Installing {0} v{1} - stopping a running copy", "[1/6] Diegiama {0} v{1} – stabdoma veikianti kopija"),
    ("[2/6] Preparing the install folder", "[2/6] Ruošiamas diegimo aplankas"),
    ("{0} already exists", "{0} jau yra"),
    ("Cannot create {0}: {1}", "Nepavyko sukurti {0}: {1}"),
    ("Created {0}", "Sukurta {0}"),
    ("[3/6] Installing the program file", "[3/6] Diegiamas programos failas"),
    ("old: {0}", "senas: {0}"),
    ("[4/6] Saving install information", "[4/6] Įrašoma diegimo informacija"),
    ("[5/6] Creating shortcuts (desktop and Start menu)", "[5/6] Kuriamos nuorodos (darbalaukyje ir meniu Pradžia)"),
    ("[6/6] Autostart", "[6/6] Automatinis paleidimas"),
    ("{0} does not run in the background - no autostart needed.", "{0} neveikia fone – automatinis paleidimas nereikalingas."),
    ("Autostart was on - keeping it (path refreshed).", "Automatinis paleidimas buvo įjungtas – paliekamas (kelias atnaujintas)."),
    ("Autostart was off - keeping it off.", "Automatinis paleidimas buvo išjungtas – paliekamas išjungtas."),
    (
        "Add {0} to Windows autostart (start automatically when you sign in)?",
        "Įtraukti {0} į Windows automatinį paleidimą (paleisti prisijungus)?",
    ),
    ("FILES UPDATED / CREATED", "ATNAUJINTI / SUKURTI FAILAI"),
    ("FILES CREATED", "SUKURTI FAILAI"),
    ("Start {0} now?", "Paleisti {0} dabar?"),
    (
        "Look at the taskbar, next to the clock. Right-click it for Refresh / Exit.",
        "Pažiūrėkite į užduočių juostą šalia laikrodžio. Dešiniuoju pelės mygtuku – meniu (atnaujinti, kalba, išeiti).",
    ),
    ("Looking for autostart entries that start {0}", "Ieškoma automatinio paleidimo įrašų, paleidžiančių {0}"),
    ("Scanning {0}", "Tikrinama {0}"),
    ("(key does not exist)", "(rakto nėra)"),
    ("Found: \"{0}\" -> {1}", "Rasta: \"{0}\" -> {1}"),
    ("Removed \"{0}\" from {1}", "Pašalinta \"{0}\" iš {1}"),
    (
        "Cannot remove \"{0}\" from {1}: run setup as administrator.",
        "Nepavyko pašalinti \"{0}\" iš {1}: paleiskite diegimo programą administratoriaus teisėmis.",
    ),
    ("Failed to remove \"{0}\"", "Nepavyko pašalinti \"{0}\""),
    ("Scanning HKCU\\{0} (Task Manager startup state)", "Tikrinama HKCU\\{0} (Užduočių tvarkytuvės paleisties būsena)"),
    ("Removed startup state \"{0}\"", "Pašalinta paleisties būsena \"{0}\""),
    ("no entry", "įrašo nėra"),
    ("Scanning Startup folder {0}", "Tikrinamas paleisties aplankas {0}"),
    ("Deleted startup shortcut {0}", "Ištrinta paleisties nuoroda {0}"),
    ("Could not delete {0}: {1}", "Nepavyko ištrinti {0}: {1}"),
    ("{0} (not present)", "{0} (nėra)"),
    ("Deleted {0} ({1})", "Ištrinta {0} ({1})"),
    ("Could not delete {0}", "Nepavyko ištrinti {0}"),
    ("Removed empty folder {0}", "Pašalintas tuščias aplankas {0}"),
    ("kept folder (not empty): {0}", "aplankas paliktas (ne tuščias): {0}"),
    ("Removed leftovers of an old BatteryHub 2 test build.", "Pašalinti senos BatteryHub 2 bandomosios versijos likučiai."),
    ("none found", "nieko nerasta"),
    ("[1/6] Uninstalling {0} - stopping a running copy", "[1/6] Šalinama {0} – stabdoma veikianti kopija"),
    ("[2/6] Checking autostart (whether it added itself anywhere)", "[2/6] Tikrinamas automatinis paleidimas"),
    ("[3/6] Removing shortcuts", "[3/6] Šalinamos nuorodos"),
    ("[4/6] Removing program files", "[4/6] Šalinami programos failai"),
    ("[5/6] Removing settings", "[5/6] Šalinami nustatymai"),
    ("(no settings stored)", "(nustatymų nėra)"),
    ("Deleted empty HKCU\\{0}", "Ištrintas tuščias HKCU\\{0}"),
    ("kept HKCU\\{0} (not empty, or already gone)", "HKCU\\{0} paliktas (ne tuščias arba jau pašalintas)"),
    ("[6/6] Cleaning up leftovers", "[6/6] Valomi likučiai"),
    ("none", "nėra"),
    ("REMOVED", "PAŠALINTA"),
    ("THANK YOU", "AČIŪ"),
    ("  Thanks for your trust in BatteryHub!", "  Ačiū, kad pasitikite BatteryHub!"),
    (
        "  If you like it, please give the project a star on GitHub - it really helps:",
        "  Jei patinka, įvertinkite projektą žvaigždute GitHub – tai tikrai padeda:",
    ),
    ("\r  This window closes in {0} s...  ", "\r  Šis langas užsidarys po {0} s...  "),
    ("{0} {1} - Setup", "{0} {1} – diegimas"),
    ("{0}   (per-user, no administrator rights needed)", "{0}   (vartotojui, administratoriaus teisių nereikia)"),
    ("WHAT DO YOU WANT TO DO?", "KĄ NORITE DARYTI?"),
    (
        "[1] Reinstall   - write v{0} again (repair files and shortcuts)",
        "[1] Įdiegti iš naujo – v{0} dar kartą (pataisyti failus ir nuorodas)",
    ),
    ("[1] Install     - fresh install of v{0}", "[1] Įdiegti     – nauja v{0} įdiegtis"),
    ("[1] Install     - install v{0} to {1}", "[1] Įdiegti     – įdiegti v{0} į {1}"),
    (
        "[2] Update      - v{0} -> v{1} (keeps your settings and autostart)",
        "[2] Atnaujinti  – v{0} -> v{1} (nustatymai ir automatinis paleidimas išlieka)",
    ),
    (
        "[2] Update      - not available: v{0} is already up to date",
        "[2] Atnaujinti  – negalima: v{0} jau naujausia",
    ),
    ("[2] Update      - not available: not installed yet", "[2] Atnaujinti  – negalima: dar neįdiegta"),
    ("[3] Uninstall   - remove {0} from this PC", "[3] Pašalinti   – pašalinti {0} iš šio kompiuterio"),
    ("[3] Uninstall   - not available: not installed", "[3] Pašalinti   – negalima: neįdiegta"),
    ("[0] Exit", "[0] Išeiti"),
    ("Nothing to update: not installed.", "Nėra ką atnaujinti: neįdiegta."),
    ("Really uninstall {0}?", "Tikrai pašalinti {0}?"),
    ("Cancelled. Nothing was changed.", "Atšaukta. Niekas nepakeista."),
    ("Exit. Nothing was changed.", "Išeinama. Niekas nepakeista."),
    ("RESULT", "REZULTATAS"),
    ("{0} is now {1}", "{0} dabar {1}"),
    ("installed (v{0}) in {1}", "įdiegta (v{0}) aplanke {1}"),
    ("Setup did not complete. See the messages above.", "Diegimas nebaigtas. Žr. pranešimus aukščiau."),
    // ---- install.rs: stopping a running copy ----
    ("Looking for a running {0} (window class \"{1}\")", "Ieškoma veikiančios {0} (lango klasė \"{1}\")"),
    ("{0} is running (window {1}), asking it to exit...", "{0} veikia (langas {1}), prašoma užsidaryti..."),
    ("{0} exited after {1} ms.", "{0} užsidarė po {1} ms."),
    ("{0} did not exit in time, terminating {1}...", "{0} laiku neužsidarė, nutraukiamas {1}..."),
    ("Process terminated.", "Procesas nutrauktas."),
    ("Could not terminate the process.", "Nepavyko nutraukti proceso."),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(s: &str) -> Vec<usize> {
        (0..10).filter(|i| s.contains(&format!("{{{i}}}"))).collect()
    }

    #[test]
    fn translations_keep_placeholders() {
        for (en, lt) in LT {
            assert_eq!(placeholders(en), placeholders(lt), "placeholders differ: {en:?}");
            assert!(!lt.is_empty(), "empty translation: {en:?}");
        }
    }

    #[test]
    fn no_duplicate_keys() {
        for (i, (a, _)) in LT.iter().enumerate() {
            assert!(LT[i + 1..].iter().all(|(b, _)| a != b), "duplicate: {a:?}");
        }
    }

    #[test]
    fn codes_round_trip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
        assert_eq!(Lang::from_code("ru"), None);
    }

    #[test]
    fn fill_replaces_every_placeholder() {
        assert_eq!(fill("{0} -> {1}, {0}", &[&"a", &2]), "a -> 2, a");
        assert_eq!(lookup("Exit"), Some("Išeiti"));
    }
}

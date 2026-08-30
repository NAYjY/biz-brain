//! T11: server-side i18n for the Owner dashboard chrome.
//!
//! Design:
//!   - Locale resolved per-request from cookie → Accept-Language → default "en"
//!   - Strings are a flat &'static str key → &'static str value map per locale
//!   - Adding a new locale: add a branch in `translations()` and fill the map
//!   - No i18n crate — just HashMaps. Graduate to Fluent if pluralisation needed.
//!
//! Usage:
//!   let t = locale_from_request(&jar, &headers);
//!   let label = t.get("nav.orders");   // → "Orders" or "คำสั่งงาน"

use std::collections::HashMap;

use axum::http::HeaderMap;
use axum_extra::extract::CookieJar;

// ── Supported locales ─────────────────────────────────────────────────────── //

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    Th,
}

impl Locale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Th => "th",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "en" | "en-us" | "en-gb" | "en-au" => Some(Self::En),
            "th" | "th-th" => Some(Self::Th),
            _ => None,
        }
    }

    /// Parse the Accept-Language header and return the best supported locale.
    /// Ignores quality weights — first match wins (sufficient for two locales).
    pub fn from_accept_language(header: &str) -> Option<Self> {
        for part in header.split(',') {
            let tag = part.split(';').next().unwrap_or("").trim();
            if let Some(locale) = Self::from_str(tag) {
                return Some(locale);
            }
            // Also try just the primary subtag ("th-TH" → "th")
            if let Some(primary) = tag.split('-').next() {
                if let Some(locale) = Self::from_str(primary) {
                    return Some(locale);
                }
            }
        }
        None
    }
}

// ── Per-request locale resolution ─────────────────────────────────────────── //

/// Resolve locale: cookie → Accept-Language → default English.
pub fn locale_from_request(jar: &CookieJar, headers: &HeaderMap) -> Translations {
    // 1. Cookie (set by POST /locale switcher)
    if let Some(cookie) = jar.get("locale") {
        if let Some(locale) = Locale::from_str(cookie.value()) {
            return Translations::new(locale);
        }
    }

    // 2. Accept-Language header
    if let Some(al) = headers.get("accept-language").and_then(|v| v.to_str().ok()) {
        if let Some(locale) = Locale::from_accept_language(al) {
            return Translations::new(locale);
        }
    }

    // 3. Default
    Translations::new(Locale::En)
}

// ── Translation lookup ────────────────────────────────────────────────────── //

pub struct Translations {
    pub locale: Locale,
    map: &'static HashMap<&'static str, &'static str>,
}

impl Translations {
    pub fn new(locale: Locale) -> Self {
        Self { locale, map: translations(locale) }
    }

    /// Look up a key. Falls back to the key itself so missing translations are
    /// visible rather than silent.
    pub fn get(&self, key: &str) -> &'static str {
        if let Some(val) = self.map.get(key).copied() {
            return val;
        }
        // Fall back to English if key is missing in the target locale
        if let Some(val) = translations(Locale::En).get(key).copied() {
            return val;
        }
        // Last resort: return the key itself — but key is &str not &'static str,
        // so we leak a tiny allocation. This path only fires for genuinely
        // missing keys (a programming error), never in normal operation.
        Box::leak(key.to_string().into_boxed_str())
    }

    /// Locale code for HTML lang attribute and the switcher <select> value.
    pub fn lang(&self) -> &'static str {
        self.locale.as_str()
    }
}

// ── Language switcher HTML fragment ──────────────────────────────────────── //

/// Renders the `<form>` + `<select>` language switcher for the topbar.
/// The form POSTs to /locale which sets the cookie and redirects back.
pub fn locale_switcher_html(current: Locale, redirect_to: &str) -> String {
    let en_sel = if current == Locale::En { " selected" } else { "" };
    let th_sel = if current == Locale::Th { " selected" } else { "" };

    format!(
        r#"<form method="POST" action="/locale" style="display:inline;">
  <input type="hidden" name="redirect_to" value="{redirect_to}">
  <select name="locale" class="locale-switcher"
          onchange="this.form.submit()"
          style="background:var(--color-surface-2);border:1px solid var(--color-border);
                 border-radius:var(--radius-sm);color:var(--color-text);
                 font-family:var(--font-body);font-size:var(--text-sm);
                 padding:var(--space-1) var(--space-2);cursor:pointer;">
    <option value="en"{en_sel}>🇬🇧 English</option>
    <option value="th"{th_sel}>🇹🇭 ไทย</option>
  </select>
</form>"#,
        redirect_to = redirect_to,
        en_sel = en_sel,
        th_sel = th_sel,
    )
}

// ── String tables ─────────────────────────────────────────────────────────── //

fn translations(locale: Locale) -> &'static HashMap<&'static str, &'static str> {
    use std::sync::OnceLock;
    static EN: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    static TH: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

    match locale {
        Locale::En => EN.get_or_init(english),
        Locale::Th => TH.get_or_init(thai),
    }
}

fn english() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();

    // ── App chrome ────────────────────────────────────────────────────── //
    m.insert("app.name", "Biz·Brain");

    // ── Topbar nav ────────────────────────────────────────────────────── //
    m.insert("nav.orders",           "Orders");
    m.insert("nav.supply",           "Supply");
    m.insert("nav.workers",          "Workers");
    m.insert("nav.suppliers",        "Suppliers");
    m.insert("nav.pending_bindings", "Pending Bindings");
    m.insert("nav.account",          "Account");
    m.insert("nav.sign_out",         "Sign out");
    m.insert("nav.branches",         "Branches");

    // ── Common buttons ────────────────────────────────────────────────── //
    m.insert("btn.create",         "Create");
    m.insert("btn.cancel",         "Cancel");
    m.insert("btn.confirm",        "Confirm");
    m.insert("btn.save",           "Save");
    m.insert("btn.remove",         "Remove");
    m.insert("btn.reject",         "Reject");
    m.insert("btn.close",          "Close");
    m.insert("btn.send",           "Send");
    m.insert("btn.add",            "Add");
    m.insert("btn.edit",           "Edit");
    m.insert("btn.delete",         "Delete");
    m.insert("btn.sign_in",        "Sign in");
    m.insert("btn.saving",         "Saving…");
    m.insert("btn.adding",         "Adding…");
    m.insert("btn.sending",        "Sending…");

    // ── Login page ────────────────────────────────────────────────────── //
    m.insert("login.title",          "Sign in — Biz-Brain");
    m.insert("login.email",          "Email");
    m.insert("login.password",       "Password");
    m.insert("login.error.invalid",  "Invalid email or password.");

    // ── Branches page ─────────────────────────────────────────────────── //
    m.insert("branches.title",              "Branches");
    m.insert("branches.page_title",         "Branches — Biz-Brain");
    m.insert("branches.none_owner",         "No branches yet — create your first one below.");
    m.insert("branches.none_manager",       "You have no branch access yet. Ask the Owner to grant you access.");
    m.insert("branches.create.heading",     "Create a branch");
    m.insert("branches.create.desc",        "Each branch has its own Workers, Suppliers, Orders, and AI agent. Use branches for separate locations, franchises, or business units.");
    m.insert("branches.create.label",       "Branch name");
    m.insert("branches.create.placeholder", "e.g. Bangkok North, Warehouse 2, Main Office");
    m.insert("branches.create.btn",         "Create");
    m.insert("branches.error.name_required","Branch name is required.");

    // ── Orders page ──────────────────────────────────────────────────── //
    m.insert("orders.title",              "Orders");
    m.insert("orders.page_title",         "Orders — Biz-Brain");
    m.insert("orders.new",                "+ New Order");
    m.insert("orders.empty",              "No orders yet. Create one to get started.");
    m.insert("orders.col.state",          "State");
    m.insert("orders.col.description",    "Description");
    m.insert("orders.col.customer",       "Customer");
    m.insert("orders.col.worker",         "Worker");
    m.insert("orders.col.actions",        "Actions");
    m.insert("orders.filter.state",       "State");
    m.insert("orders.filter.worker",      "Worker");
    m.insert("orders.filter.search",      "Search");
    m.insert("orders.filter.placeholder", "Description or job name…");
    m.insert("orders.filter.all_states",  "All states");
    m.insert("orders.filter.all_workers", "All workers");
    m.insert("orders.filter.clear",       "Clear");
    m.insert("orders.all_loaded",         "All orders loaded.");
    m.insert("orders.loading",            "Loading…");

    // ── Create order modal ────────────────────────────────────────────── //
    m.insert("orders.create.title",           "New Order");
    m.insert("orders.create.customer",        "Customer");
    m.insert("orders.create.new_customer",    "+ New");
    m.insert("orders.create.new_customer_name","New customer name");
    m.insert("orders.create.customer_placeholder","Customer name");
    m.insert("orders.create.short_name",      "Job name");
    m.insert("orders.create.short_name_hint", "(optional, max 20 chars)");
    m.insert("orders.create.short_name_placeholder","e.g. AC-B3");
    m.insert("orders.create.description",     "Description");
    m.insert("orders.create.description_placeholder","What needs doing?");
    m.insert("orders.create.btn",             "Create");

    // ── Assign worker modal ───────────────────────────────────────────── //
    m.insert("orders.assign.title",    "Assign Worker");
    m.insert("orders.reassign.title",  "Reassign Worker");
    m.insert("orders.assign.label",    "Worker");
    m.insert("orders.assign.placeholder","Select worker…");
    m.insert("orders.assign.btn",      "Confirm");

    // ── Order state pills ─────────────────────────────────────────────── //
    m.insert("state.unassigned",           "UNASSIGNED");
    m.insert("state.assigned",             "ASSIGNED");
    m.insert("state.accepted",             "ACCEPTED");
    m.insert("state.pending_clarification","PENDING CLARIFICATION");
    m.insert("state.unavailable",          "UNAVAILABLE");
    m.insert("state.ready_for_pickup",     "READY FOR PICKUP");
    m.insert("state.done",                 "DONE");
    m.insert("state.cancelled",            "CANCELLED");
    m.insert("state.draft",                "DRAFT");
    m.insert("state.sent",                 "SENT");
    m.insert("state.invoice_received",     "INVOICE RECEIVED");
    m.insert("state.owner_approved_invoice","OWNER APPROVED INVOICE");
    m.insert("state.supplier_confirmed",   "SUPPLIER CONFIRMED");

    // ── Nudge banner ──────────────────────────────────────────────────── //
    m.insert("nudge.no_job_names", "active orders without job names — workers may struggle to identify them. Set job names via ⚙️ → Set job name.");

    // ── Supply requests page ──────────────────────────────────────────── //
    m.insert("supply.title",          "Supply Requests");
    m.insert("supply.page_title",     "Supply Requests — Biz-Brain");
    m.insert("supply.new",            "+ New Request");
    m.insert("supply.empty",          "No supply requests yet.");
    m.insert("supply.col.state",      "State");
    m.insert("supply.col.description","Description");
    m.insert("supply.col.orders",     "Orders");
    m.insert("supply.col.actions",    "Actions");
    m.insert("supply.filter.state",   "State");
    m.insert("supply.filter.all",     "All states");
    m.insert("supply.all_loaded",     "All supply requests loaded.");

    // ── Create supply request modal ───────────────────────────────────── //
    m.insert("supply.create.title",           "New Supply Request");
    m.insert("supply.create.description",     "Description");
    m.insert("supply.create.description_ph",  "What supplies are needed?");
    m.insert("supply.create.orders",          "In-flight Orders to cover");
    m.insert("supply.create.orders_hint",     "Hold Ctrl/Cmd to select multiple");
    m.insert("supply.create.btn",             "Create");

    // ── Approve invoice modal ─────────────────────────────────────────── //
    m.insert("supply.approve.title",   "Approve Invoice");
    m.insert("supply.approve.warning", "Approving an invoice is a financial commitment. Review carefully.");
    m.insert("supply.approve.label",   "Invoice");
    m.insert("supply.approve.btn",     "Approve Invoice");

    // ── Workers page ──────────────────────────────────────────────────── //
    m.insert("workers.title",       "Workers");
    m.insert("workers.page_title",  "Workers — Biz-Brain");
    m.insert("workers.add",         "+ Add Worker");
    m.insert("workers.empty",       "No workers yet. Create one to get started.");
    m.insert("workers.col.name",    "Name");
    m.insert("workers.col.channel", "Channel");
    m.insert("workers.col.sender",  "Sender ID");
    m.insert("workers.col.actions", "Actions");
    m.insert("workers.not_bound",   "Not bound");

    // ── Worker onboarding card ────────────────────────────────────────── //
    m.insert("workers.onboarding.heading", "How Worker onboarding works");
    m.insert("workers.onboarding.step1",   "Add a Worker here (name only — creates their profile)");
    m.insert("workers.onboarding.step2",   "Tell the Worker to send any message to your LINE bot");
    m.insert("workers.onboarding.step3_pre", "Go to the");
    m.insert("workers.onboarding.step3_link","Workers & Suppliers");
    m.insert("workers.onboarding.step3_post","page — their message appears as a pending binding");
    m.insert("workers.onboarding.step4",   "Confirm the binding to link their LINE account to this Worker profile");

    // ── Create worker modal ────────────────────────────────────────────── //
    m.insert("workers.create.title",       "Add Worker");
    m.insert("workers.create.name_label",  "Name");
    m.insert("workers.create.name_ph",     "e.g. Somchai K.");
    m.insert("workers.create.hint",        "After adding, tell this worker to message your LINE bot. Their binding will appear on the Workers & Suppliers page for you to confirm.");
    m.insert("workers.create.btn",         "Add Worker");

    // ── Suppliers page ────────────────────────────────────────────────── //
    m.insert("suppliers.title",       "Suppliers");
    m.insert("suppliers.page_title",  "Suppliers — Biz-Brain");
    m.insert("suppliers.add",         "+ Add Supplier");
    m.insert("suppliers.empty",       "No suppliers yet. Create one to get started.");
    m.insert("suppliers.col.name",    "Name");
    m.insert("suppliers.col.channel", "Channel");
    m.insert("suppliers.col.sender",  "Sender ID");
    m.insert("suppliers.col.actions", "Actions");
    m.insert("suppliers.not_bound",   "Not bound");

    // ── Supplier onboarding card ──────────────────────────────────────── //
    m.insert("suppliers.onboarding.heading","How Supplier onboarding works");
    m.insert("suppliers.onboarding.step1",  "Add a Supplier here (name only — creates their profile)");
    m.insert("suppliers.onboarding.step2",  "Tell the Supplier to send any message to your WhatsApp bot");
    m.insert("suppliers.onboarding.step3_pre","Go to the");
    m.insert("suppliers.onboarding.step3_link","Workers & Suppliers");
    m.insert("suppliers.onboarding.step3_post","page — their message appears as a pending binding");
    m.insert("suppliers.onboarding.step4",  "Confirm the binding to link their WhatsApp account to this Supplier profile");

    // ── Create supplier modal ─────────────────────────────────────────── //
    m.insert("suppliers.create.title",      "Add Supplier");
    m.insert("suppliers.create.name_label", "Name");
    m.insert("suppliers.create.name_ph",    "e.g. ABC Hardware Co.");
    m.insert("suppliers.create.hint",       "After adding, tell this supplier to message your WhatsApp bot. Their binding will appear on the Workers & Suppliers page for you to confirm.");
    m.insert("suppliers.create.btn",        "Add Supplier");

    // ── Actors (pending bindings) page ────────────────────────────────── //
    m.insert("actors.title",       "Workers & Suppliers");
    m.insert("actors.page_title",  "Workers & Suppliers — Biz-Brain");
    m.insert("actors.subtitle",    "Confirm a binding to trust that sender. Reject to remove it — their next message creates a new pending entry.");
    m.insert("actors.empty",       "No pending bindings. When a new Worker or Supplier messages for the first time, they appear here.");
    m.insert("actors.col.channel", "Channel");
    m.insert("actors.col.sender",  "Sender ID");
    m.insert("actors.col.type",    "Type");
    m.insert("actors.col.seen",    "First seen");
    m.insert("actors.col.actions", "Actions");
    m.insert("actors.type.worker",   "Worker");
    m.insert("actors.type.supplier", "Supplier");
    m.insert("actors.btn.confirm",   "Confirm");
    m.insert("actors.btn.reject",    "Reject");

    // ── Account settings page ─────────────────────────────────────────── //
    m.insert("account.title",           "Account settings");
    m.insert("account.page_title",      "Account Settings — Biz-Brain");
    m.insert("account.role",            "Role");
    m.insert("account.role.owner",      "Owner");
    m.insert("account.role.manager",    "Manager");
    m.insert("account.change_password", "Change password");
    m.insert("account.current_pw",      "Current password");
    m.insert("account.new_pw",          "New password");
    m.insert("account.new_pw_hint",     "Minimum 8 characters.");
    m.insert("account.confirm_pw",      "Confirm new password");
    m.insert("account.save_pw",         "Save new password");
    m.insert("account.saving_pw",       "Saving…");
    m.insert("account.pw_success",      "Password changed. Other sessions have been signed out.");
    m.insert("account.back",            "← Dashboard");
    m.insert("account.error.fields_required",  "All fields are required.");
    m.insert("account.error.too_short",        "New password must be at least 8 characters.");
    m.insert("account.error.no_match",         "New passwords do not match.");
    m.insert("account.error.network",          "Network error — please try again.");

    // ── Thread modal ──────────────────────────────────────────────────── //
    m.insert("thread.reply_placeholder", "Reply to worker…");
    m.insert("thread.empty",             "No messages yet.");
    m.insert("thread.send",              "Send");
    m.insert("thread.label.worker",      "Worker");
    m.insert("thread.label.owner",       "Bot / Owner");

    // ── Connecting / live badge ───────────────────────────────────────── //
    m.insert("live.connecting",    "Connecting…");
    m.insert("live.live",          "Live");
    m.insert("live.reconnecting",  "Reconnecting…");

    m
}

fn thai() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();

    // ── App chrome ────────────────────────────────────────────────────── //
    m.insert("app.name", "Biz·Brain");

    // ── Topbar nav ────────────────────────────────────────────────────── //
    m.insert("nav.orders",           "คำสั่งงาน");
    m.insert("nav.supply",           "จัดซื้อ");
    m.insert("nav.workers",          "พนักงาน");
    m.insert("nav.suppliers",        "ซัพพลายเออร์");
    m.insert("nav.pending_bindings", "รอยืนยันตัวตน");
    m.insert("nav.account",          "บัญชี");
    m.insert("nav.sign_out",         "ออกจากระบบ");
    m.insert("nav.branches",         "สาขา");

    // ── Common buttons ────────────────────────────────────────────────── //
    m.insert("btn.create",         "สร้าง");
    m.insert("btn.cancel",         "ยกเลิก");
    m.insert("btn.confirm",        "ยืนยัน");
    m.insert("btn.save",           "บันทึก");
    m.insert("btn.remove",         "ลบ");
    m.insert("btn.reject",         "ปฏิเสธ");
    m.insert("btn.close",          "ปิด");
    m.insert("btn.send",           "ส่ง");
    m.insert("btn.add",            "เพิ่ม");
    m.insert("btn.edit",           "แก้ไข");
    m.insert("btn.delete",         "ลบ");
    m.insert("btn.sign_in",        "เข้าสู่ระบบ");
    m.insert("btn.saving",         "กำลังบันทึก…");
    m.insert("btn.adding",         "กำลังเพิ่ม…");
    m.insert("btn.sending",        "กำลังส่ง…");

    // ── Login page ────────────────────────────────────────────────────── //
    m.insert("login.title",          "เข้าสู่ระบบ — Biz-Brain");
    m.insert("login.email",          "อีเมล");
    m.insert("login.password",       "รหัสผ่าน");
    m.insert("login.error.invalid",  "อีเมลหรือรหัสผ่านไม่ถูกต้อง");

    // ── Branches page ─────────────────────────────────────────────────── //
    m.insert("branches.title",              "สาขา");
    m.insert("branches.page_title",         "สาขา — Biz-Brain");
    m.insert("branches.none_owner",         "ยังไม่มีสาขา — สร้างสาขาแรกของคุณด้านล่าง");
    m.insert("branches.none_manager",       "คุณยังไม่ได้รับสิทธิ์เข้าถึงสาขาใด กรุณาติดต่อเจ้าของระบบ");
    m.insert("branches.create.heading",     "สร้างสาขา");
    m.insert("branches.create.desc",        "แต่ละสาขามีพนักงาน ซัพพลายเออร์ คำสั่งงาน และ AI agent เป็นของตัวเอง ใช้สาขาสำหรับสถานที่ต่างๆ แฟรนไชส์ หรือหน่วยธุรกิจ");
    m.insert("branches.create.label",       "ชื่อสาขา");
    m.insert("branches.create.placeholder", "เช่น กรุงเทพเหนือ, โกดัง 2, สำนักงานใหญ่");
    m.insert("branches.create.btn",         "สร้าง");
    m.insert("branches.error.name_required","กรุณากรอกชื่อสาขา");

    // ── Orders page ──────────────────────────────────────────────────── //
    m.insert("orders.title",              "คำสั่งงาน");
    m.insert("orders.page_title",         "คำสั่งงาน — Biz-Brain");
    m.insert("orders.new",                "+ สร้างคำสั่งงาน");
    m.insert("orders.empty",              "ยังไม่มีคำสั่งงาน สร้างคำสั่งงานแรกของคุณเลย");
    m.insert("orders.col.state",          "สถานะ");
    m.insert("orders.col.description",    "รายละเอียด");
    m.insert("orders.col.customer",       "ลูกค้า");
    m.insert("orders.col.worker",         "พนักงาน");
    m.insert("orders.col.actions",        "จัดการ");
    m.insert("orders.filter.state",       "สถานะ");
    m.insert("orders.filter.worker",      "พนักงาน");
    m.insert("orders.filter.search",      "ค้นหา");
    m.insert("orders.filter.placeholder", "รายละเอียดหรือชื่องาน…");
    m.insert("orders.filter.all_states",  "ทุกสถานะ");
    m.insert("orders.filter.all_workers", "พนักงานทั้งหมด");
    m.insert("orders.filter.clear",       "ล้างตัวกรอง");
    m.insert("orders.all_loaded",         "โหลดคำสั่งงานทั้งหมดแล้ว");
    m.insert("orders.loading",            "กำลังโหลด…");

    // ── Create order modal ────────────────────────────────────────────── //
    m.insert("orders.create.title",            "สร้างคำสั่งงาน");
    m.insert("orders.create.customer",         "ลูกค้า");
    m.insert("orders.create.new_customer",     "+ เพิ่มใหม่");
    m.insert("orders.create.new_customer_name","ชื่อลูกค้าใหม่");
    m.insert("orders.create.customer_placeholder","ชื่อลูกค้า");
    m.insert("orders.create.short_name",       "ชื่องาน");
    m.insert("orders.create.short_name_hint",  "(ไม่บังคับ, สูงสุด 20 ตัวอักษร)");
    m.insert("orders.create.short_name_placeholder","เช่น AC-B3");
    m.insert("orders.create.description",      "รายละเอียด");
    m.insert("orders.create.description_placeholder","งานนี้ต้องทำอะไร?");
    m.insert("orders.create.btn",              "สร้าง");

    // ── Assign worker modal ───────────────────────────────────────────── //
    m.insert("orders.assign.title",     "มอบหมายพนักงาน");
    m.insert("orders.reassign.title",   "เปลี่ยนพนักงาน");
    m.insert("orders.assign.label",     "พนักงาน");
    m.insert("orders.assign.placeholder","เลือกพนักงาน…");
    m.insert("orders.assign.btn",       "ยืนยัน");

    // ── Order state pills ─────────────────────────────────────────────── //
    m.insert("state.unassigned",            "ยังไม่มอบหมาย");
    m.insert("state.assigned",              "มอบหมายแล้ว");
    m.insert("state.accepted",              "รับงานแล้ว");
    m.insert("state.pending_clarification", "รอคำชี้แจง");
    m.insert("state.unavailable",           "ไม่ว่าง");
    m.insert("state.ready_for_pickup",      "พร้อมรับของ");
    m.insert("state.done",                  "เสร็จสิ้น");
    m.insert("state.cancelled",             "ยกเลิก");
    m.insert("state.draft",                 "แบบร่าง");
    m.insert("state.sent",                  "ส่งแล้ว");
    m.insert("state.invoice_received",      "ได้รับใบแจ้งราคา");
    m.insert("state.owner_approved_invoice","เจ้าของอนุมัติแล้ว");
    m.insert("state.supplier_confirmed",    "ซัพพลายเออร์ยืนยันแล้ว");

    // ── Nudge banner ──────────────────────────────────────────────────── //
    m.insert("nudge.no_job_names", "คำสั่งงานที่ยังไม่มีชื่องาน — พนักงานอาจระบุงานได้ยาก กรุณาตั้งชื่องานผ่าน ⚙️ → ตั้งชื่องาน");

    // ── Supply requests page ──────────────────────────────────────────── //
    m.insert("supply.title",           "คำขอจัดซื้อ");
    m.insert("supply.page_title",      "คำขอจัดซื้อ — Biz-Brain");
    m.insert("supply.new",             "+ สร้างคำขอจัดซื้อ");
    m.insert("supply.empty",           "ยังไม่มีคำขอจัดซื้อ");
    m.insert("supply.col.state",       "สถานะ");
    m.insert("supply.col.description", "รายละเอียด");
    m.insert("supply.col.orders",      "คำสั่งงาน");
    m.insert("supply.col.actions",     "จัดการ");
    m.insert("supply.filter.state",    "สถานะ");
    m.insert("supply.filter.all",      "ทุกสถานะ");
    m.insert("supply.all_loaded",      "โหลดคำขอจัดซื้อทั้งหมดแล้ว");

    // ── Create supply request modal ───────────────────────────────────── //
    m.insert("supply.create.title",          "สร้างคำขอจัดซื้อ");
    m.insert("supply.create.description",    "รายละเอียด");
    m.insert("supply.create.description_ph", "ต้องการวัสดุอะไร?");
    m.insert("supply.create.orders",         "คำสั่งงานที่เกี่ยวข้อง");
    m.insert("supply.create.orders_hint",    "กด Ctrl/Cmd ค้างเพื่อเลือกหลายรายการ");
    m.insert("supply.create.btn",            "สร้าง");

    // ── Approve invoice modal ─────────────────────────────────────────── //
    m.insert("supply.approve.title",   "อนุมัติใบแจ้งราคา");
    m.insert("supply.approve.warning", "การอนุมัติใบแจ้งราคาถือเป็นการผูกมัดทางการเงิน กรุณาตรวจสอบให้รอบคอบ");
    m.insert("supply.approve.label",   "ใบแจ้งราคา");
    m.insert("supply.approve.btn",     "อนุมัติใบแจ้งราคา");

    // ── Workers page ──────────────────────────────────────────────────── //
    m.insert("workers.title",       "พนักงาน");
    m.insert("workers.page_title",  "พนักงาน — Biz-Brain");
    m.insert("workers.add",         "+ เพิ่มพนักงาน");
    m.insert("workers.empty",       "ยังไม่มีพนักงาน สร้างพนักงานแรกของคุณเลย");
    m.insert("workers.col.name",    "ชื่อ");
    m.insert("workers.col.channel", "ช่องทาง");
    m.insert("workers.col.sender",  "รหัสผู้ส่ง");
    m.insert("workers.col.actions", "จัดการ");
    m.insert("workers.not_bound",   "ยังไม่ผูก");

    // ── Worker onboarding card ────────────────────────────────────────── //
    m.insert("workers.onboarding.heading", "วิธีเพิ่มพนักงาน");
    m.insert("workers.onboarding.step1",   "เพิ่มพนักงานที่นี่ (ชื่อเท่านั้น — สร้างโปรไฟล์ให้)");
    m.insert("workers.onboarding.step2",   "บอกให้พนักงานส่งข้อความหาบอท LINE ของคุณ");
    m.insert("workers.onboarding.step3_pre","ไปที่หน้า");
    m.insert("workers.onboarding.step3_link","พนักงาน & ซัพพลายเออร์");
    m.insert("workers.onboarding.step3_post","— ข้อความของพวกเขาจะปรากฏเป็นคำขอยืนยันตัวตน");
    m.insert("workers.onboarding.step4",   "ยืนยันการผูกบัญชีเพื่อเชื่อมโยง LINE กับโปรไฟล์พนักงาน");

    // ── Create worker modal ────────────────────────────────────────────── //
    m.insert("workers.create.title",       "เพิ่มพนักงาน");
    m.insert("workers.create.name_label",  "ชื่อ");
    m.insert("workers.create.name_ph",     "เช่น สมชาย ก.");
    m.insert("workers.create.hint",        "หลังจากเพิ่มแล้ว บอกให้พนักงานส่งข้อความหาบอท LINE การผูกบัญชีจะปรากฏในหน้าพนักงาน & ซัพพลายเออร์");
    m.insert("workers.create.btn",         "เพิ่มพนักงาน");

    // ── Suppliers page ────────────────────────────────────────────────── //
    m.insert("suppliers.title",       "ซัพพลายเออร์");
    m.insert("suppliers.page_title",  "ซัพพลายเออร์ — Biz-Brain");
    m.insert("suppliers.add",         "+ เพิ่มซัพพลายเออร์");
    m.insert("suppliers.empty",       "ยังไม่มีซัพพลายเออร์ สร้างซัพพลายเออร์แรกของคุณเลย");
    m.insert("suppliers.col.name",    "ชื่อ");
    m.insert("suppliers.col.channel", "ช่องทาง");
    m.insert("suppliers.col.sender",  "รหัสผู้ส่ง");
    m.insert("suppliers.col.actions", "จัดการ");
    m.insert("suppliers.not_bound",   "ยังไม่ผูก");

    // ── Supplier onboarding card ──────────────────────────────────────── //
    m.insert("suppliers.onboarding.heading","วิธีเพิ่มซัพพลายเออร์");
    m.insert("suppliers.onboarding.step1",  "เพิ่มซัพพลายเออร์ที่นี่ (ชื่อเท่านั้น — สร้างโปรไฟล์ให้)");
    m.insert("suppliers.onboarding.step2",  "บอกให้ซัพพลายเออร์ส่งข้อความหาบอท WhatsApp ของคุณ");
    m.insert("suppliers.onboarding.step3_pre","ไปที่หน้า");
    m.insert("suppliers.onboarding.step3_link","พนักงาน & ซัพพลายเออร์");
    m.insert("suppliers.onboarding.step3_post","— ข้อความของพวกเขาจะปรากฏเป็นคำขอยืนยันตัวตน");
    m.insert("suppliers.onboarding.step4",  "ยืนยันการผูกบัญชีเพื่อเชื่อมโยง WhatsApp กับโปรไฟล์ซัพพลายเออร์");

    // ── Create supplier modal ─────────────────────────────────────────── //
    m.insert("suppliers.create.title",      "เพิ่มซัพพลายเออร์");
    m.insert("suppliers.create.name_label", "ชื่อ");
    m.insert("suppliers.create.name_ph",    "เช่น บริษัท ABC Hardware");
    m.insert("suppliers.create.hint",       "หลังจากเพิ่มแล้ว บอกให้ซัพพลายเออร์ส่งข้อความหาบอท WhatsApp การผูกบัญชีจะปรากฏในหน้าพนักงาน & ซัพพลายเออร์");
    m.insert("suppliers.create.btn",        "เพิ่มซัพพลายเออร์");

    // ── Actors (pending bindings) page ────────────────────────────────── //
    m.insert("actors.title",       "พนักงาน & ซัพพลายเออร์");
    m.insert("actors.page_title",  "พนักงาน & ซัพพลายเออร์ — Biz-Brain");
    m.insert("actors.subtitle",    "ยืนยันการผูกบัญชีเพื่อเชื่อถือผู้ส่งรายนั้น ปฏิเสธเพื่อลบออก — ข้อความถัดไปของพวกเขาจะสร้างรายการใหม่");
    m.insert("actors.empty",       "ไม่มีคำขอรอยืนยัน เมื่อพนักงานหรือซัพพลายเออร์ส่งข้อความครั้งแรก จะปรากฏที่นี่");
    m.insert("actors.col.channel", "ช่องทาง");
    m.insert("actors.col.sender",  "รหัสผู้ส่ง");
    m.insert("actors.col.type",    "ประเภท");
    m.insert("actors.col.seen",    "พบครั้งแรก");
    m.insert("actors.col.actions", "จัดการ");
    m.insert("actors.type.worker",   "พนักงาน");
    m.insert("actors.type.supplier", "ซัพพลายเออร์");
    m.insert("actors.btn.confirm",   "ยืนยัน");
    m.insert("actors.btn.reject",    "ปฏิเสธ");

    // ── Account settings page ─────────────────────────────────────────── //
    m.insert("account.title",           "ตั้งค่าบัญชี");
    m.insert("account.page_title",      "ตั้งค่าบัญชี — Biz-Brain");
    m.insert("account.role",            "บทบาท");
    m.insert("account.role.owner",      "เจ้าของ");
    m.insert("account.role.manager",    "ผู้จัดการ");
    m.insert("account.change_password", "เปลี่ยนรหัสผ่าน");
    m.insert("account.current_pw",      "รหัสผ่านปัจจุบัน");
    m.insert("account.new_pw",          "รหัสผ่านใหม่");
    m.insert("account.new_pw_hint",     "อย่างน้อย 8 ตัวอักษร");
    m.insert("account.confirm_pw",      "ยืนยันรหัสผ่านใหม่");
    m.insert("account.save_pw",         "บันทึกรหัสผ่านใหม่");
    m.insert("account.saving_pw",       "กำลังบันทึก…");
    m.insert("account.pw_success",      "เปลี่ยนรหัสผ่านสำเร็จ เซสชันอื่นถูกออกจากระบบแล้ว");
    m.insert("account.back",            "← แดชบอร์ด");
    m.insert("account.error.fields_required", "กรุณากรอกข้อมูลให้ครบทุกช่อง");
    m.insert("account.error.too_short",       "รหัสผ่านใหม่ต้องมีอย่างน้อย 8 ตัวอักษร");
    m.insert("account.error.no_match",        "รหัสผ่านใหม่ไม่ตรงกัน");
    m.insert("account.error.network",         "เกิดข้อผิดพลาดในการเชื่อมต่อ กรุณาลองใหม่");

    // ── Thread modal ──────────────────────────────────────────────────── //
    m.insert("thread.reply_placeholder", "ตอบกลับพนักงาน…");
    m.insert("thread.empty",             "ยังไม่มีข้อความ");
    m.insert("thread.send",              "ส่ง");
    m.insert("thread.label.worker",      "พนักงาน");
    m.insert("thread.label.owner",       "บอท / เจ้าของ");

    // ── Connecting / live badge ───────────────────────────────────────── //
    m.insert("live.connecting",   "กำลังเชื่อมต่อ…");
    m.insert("live.live",         "ออนไลน์");
    m.insert("live.reconnecting", "กำลังเชื่อมต่อใหม่…");

    m
}
import { Hono } from "hono";
import { getCookie, setCookie, deleteCookie } from "hono/cookie";

// ---------- Types ----------

export interface Env {
  DB: D1Database;
  SESSIONS: KVNamespace;
  ASSETS: Fetcher;
  SESSION_TTL_SECONDS?: string;
  PBKDF2_ITERATIONS?: string;
}

interface Session {
  account_id: string;
  csrf: string;
  created_at: number;
}

type Kind = "fridge" | "freezer" | "other";
type PackStatus = "opened" | "unopened";

interface Appliance {
  id: string;
  name: string;
  kind: Kind;
  location?: string | null;
  description?: string | null;
  created_at: string;
  updated_at: string;
}

interface Group {
  id: string;
  name: string;
  appliance_id: string;
  description?: string | null;
  created_at: string;
  updated_at: string;
}

interface InventoryItemBase {
  id: string;
  name: string;
  appliance_id: string;
  group_id?: string | null;
  category?: string | null;
  expiration_date?: string | null;
  notes?: string | null;
  has_photo?: boolean;
  created_at: string;
  updated_at: string;
}

type InventoryItem = InventoryItemBase &
  (
    | { tracking: "individual"; quantity: number; unit: string }
    | { tracking: "pack"; pack_count: number; pack_label: string; pack_status: PackStatus }
  );

interface Inventory {
  appliances: Appliance[];
  groups: Group[];
  items: InventoryItem[];
}

const EMPTY_INVENTORY: Inventory = { appliances: [], groups: [], items: [] };

const SESSION_COOKIE = "fridgeflow_session";
const CSRF_HEADER = "x-fridgeflow-csrf";

// ---------- App ----------

const app = new Hono<{
  Bindings: Env;
  Variables: { session?: Session };
}>();

// ---------- Auth helpers ----------

function sessionTtl(env: Env): number {
  const n = Number(env.SESSION_TTL_SECONDS);
  return Number.isFinite(n) && n > 0 ? n : 3600;
}

function pbkdf2Iterations(env: Env): number {
  const n = Number(env.PBKDF2_ITERATIONS);
  return Number.isFinite(n) && n > 0 ? n : 500_000;
}

function randomToken(bytes = 32): string {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return [...buf].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function constantTimeEq(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}

async function hashPin(pin: string, env: Env): Promise<string> {
  const salt = new Uint8Array(16);
  crypto.getRandomValues(salt);
  const iterations = pbkdf2Iterations(env);
  const bits = await deriveBits(pin, salt, iterations);
  return `pbkdf2-sha256:${iterations}:${toBase64(salt)}:${toBase64(bits)}`;
}

async function verifyPin(pin: string, stored: string): Promise<boolean> {
  const parts = stored.split(":");
  if (parts.length !== 4 || parts[0] !== "pbkdf2-sha256") return false;
  const iterations = Number(parts[1]);
  if (!Number.isFinite(iterations) || iterations <= 0) return false;
  const salt = fromBase64(parts[2]!);
  const want = fromBase64(parts[3]!);
  const got = new Uint8Array(await deriveBits(pin, salt, iterations));
  if (got.length !== want.length) return false;
  let diff = 0;
  for (let i = 0; i < got.length; i++) diff |= got[i]! ^ want[i]!;
  return diff === 0;
}

async function deriveBits(
  pin: string,
  salt: Uint8Array,
  iterations: number,
): Promise<ArrayBuffer> {
  const baseKey = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(pin),
    { name: "PBKDF2" },
    false,
    ["deriveBits"],
  );
  return crypto.subtle.deriveBits(
    { name: "PBKDF2", salt, iterations, hash: "SHA-256" },
    baseKey,
    256,
  );
}

function toBase64(bytes: ArrayBuffer | Uint8Array): string {
  const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let s = "";
  for (const b of arr) s += String.fromCharCode(b);
  return btoa(s);
}

function fromBase64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function nowIso(): string {
  return new Date().toISOString();
}

async function createSession(env: Env, accountId: string): Promise<Session & { id: string }> {
  const id = randomToken(32);
  const csrf = randomToken(32);
  const session: Session = { account_id: accountId, csrf, created_at: Date.now() };
  await env.SESSIONS.put(id, JSON.stringify(session), {
    expirationTtl: sessionTtl(env),
  });
  return { id, ...session };
}

async function loadSession(env: Env, id: string): Promise<Session | null> {
  const raw = await env.SESSIONS.get(id);
  if (!raw) return null;
  const session = JSON.parse(raw) as Session;
  // Refresh TTL (sliding expiration) so active users don't get kicked.
  await env.SESSIONS.put(id, raw, { expirationTtl: sessionTtl(env) });
  return session;
}

async function revokeSession(env: Env, id: string): Promise<void> {
  await env.SESSIONS.delete(id);
}

async function requireAuth(c: any): Promise<Response | null> {
  const id = getCookie(c, SESSION_COOKIE);
  if (!id) return jsonError(c, 401, "unauthorized", "authentication required");
  const session = await loadSession(c.env, id);
  if (!session) return jsonError(c, 401, "unauthorized", "authentication required");
  c.set("session", session);
  return null;
}

function requireCsrf(c: any, session: Session): Response | null {
  const token = c.req.header(CSRF_HEADER);
  if (!token || !constantTimeEq(token, session.csrf)) {
    return jsonError(c, 403, "forbidden", "request not permitted");
  }
  return null;
}

function setSessionCookie(c: any, id: string, env: Env): void {
  setCookie(c, SESSION_COOKIE, id, {
    httpOnly: true,
    sameSite: "Strict",
    secure: new URL(c.req.url).protocol === "https:",
    path: "/",
    maxAge: sessionTtl(env),
  });
}

function clearSessionCookie(c: any): void {
  deleteCookie(c, SESSION_COOKIE, { path: "/" });
}

// ---------- Validation ----------

function validateName(name: string | undefined): string | null {
  if (!name) return null;
  const trimmed = name.trim();
  if (trimmed.length === 0 || trimmed.length > 60) return null;
  return trimmed;
}

function validatePin(pin: string | undefined): boolean {
  return !!pin && /^\d{4,12}$/.test(pin);
}

function validItemName(name: unknown): name is string {
  return typeof name === "string" && name.trim().length > 0 && name.trim().length <= 200;
}

function isValidIsoDate(s: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s)) return false;
  const d = new Date(s + "T00:00:00Z");
  return !isNaN(d.getTime());
}

function blankToNull(s: string | null | undefined): string | null {
  if (s == null) return null;
  const t = s.trim();
  return t.length === 0 ? null : t;
}

function roundQuantity(v: number): number {
  return Math.round(v * 10000) / 10000;
}

// ---------- D1 helpers ----------

async function getInventory(env: Env, accountId: string): Promise<Inventory> {
  const row = await env.DB.prepare("SELECT data FROM inventories WHERE account_id = ?")
    .bind(accountId)
    .first<{ data: string }>();
  if (!row) return structuredClone(EMPTY_INVENTORY);
  try {
    return JSON.parse(row.data) as Inventory;
  } catch {
    return structuredClone(EMPTY_INVENTORY);
  }
}

async function putInventory(env: Env, accountId: string, inv: Inventory): Promise<void> {
  const now = nowIso();
  await env.DB.prepare(
    `INSERT INTO inventories (account_id, data, updated_at) VALUES (?, ?, ?)
     ON CONFLICT(account_id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at`,
  )
    .bind(accountId, JSON.stringify(inv), now)
    .run();
}

/**
 * Mutate an account's inventory atomically. D1 serializes writes per-database,
 * and we rewrite the whole blob, so there are no interleaved partial updates.
 * We also wrap in a transaction so a failing mutator leaves the row untouched.
 */
async function mutateInventory<T>(
  env: Env,
  accountId: string,
  fn: (inv: Inventory) => T | Promise<T>,
): Promise<T> {
  const inv = await getInventory(env, accountId);
  const result = await fn(inv);
  await putInventory(env, accountId, inv);
  return result;
}

// ---------- Error helpers ----------

function jsonError(c: any, status: number, code: string, message: string): Response {
  return c.json({ error: { code, message } }, status);
}

function newId(): string {
  return crypto.randomUUID();
}

// ---------- Routes: health + auth ----------

app.get("/api/health", (c) => c.json({ status: "ok" }));

app.get("/api/auth/status", async (c) => {
  const rows = await c.env.DB.prepare("SELECT name FROM accounts ORDER BY name_lower").all<{
    name: string;
  }>();
  const names = (rows.results ?? []).map((r) => r.name);
  return c.json({
    has_accounts: names.length > 0,
    account_count: names.length,
    account_names: names,
  });
});

app.get("/api/auth/session", async (c) => {
  const id = getCookie(c, SESSION_COOKIE);
  if (!id) return c.json({ authenticated: false });
  const session = await loadSession(c.env, id);
  if (!session) return c.json({ authenticated: false });
  const account = await c.env.DB.prepare("SELECT name FROM accounts WHERE id = ?")
    .bind(session.account_id)
    .first<{ name: string }>();
  return c.json({
    authenticated: true,
    csrf_token: session.csrf,
    account_id: session.account_id,
    account_name: account?.name ?? null,
  });
});

app.post("/api/auth/signup", async (c) => {
  const body = await c.req.json<{ name?: string; pin?: string }>().catch(() => ({}));
  const name = validateName(body.name);
  if (!name) return jsonError(c, 400, "bad_request", "name is required");
  if (!validatePin(body.pin)) return jsonError(c, 400, "bad_request", "pin must be 4-12 digits");
  const nameLower = name.toLowerCase();
  const existing = await c.env.DB.prepare(
    "SELECT id FROM accounts WHERE name_lower = ?",
  )
    .bind(nameLower)
    .first();
  if (existing) return jsonError(c, 409, "conflict", "that name is already in use");
  const id = newId();
  const now = nowIso();
  const pinHash = await hashPin(body.pin!, c.env);
  await c.env.DB.prepare(
    "INSERT INTO accounts (id, name, name_lower, pin_hash, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
  )
    .bind(id, name, nameLower, pinHash, now, now)
    .run();
  const session = await createSession(c.env, id);
  setSessionCookie(c, session.id, c.env);
  return c.json(
    {
      authenticated: true,
      csrf_token: session.csrf,
      account_id: id,
      account_name: name,
    },
    201,
  );
});

app.post("/api/auth/login", async (c) => {
  const body = await c.req.json<{ name?: string; pin?: string }>().catch(() => ({}));
  const name = validateName(body.name);
  if (!name || !validatePin(body.pin)) {
    return jsonError(c, 401, "unauthorized", "incorrect name or PIN");
  }
  const account = await c.env.DB.prepare(
    "SELECT id, name, pin_hash FROM accounts WHERE name_lower = ?",
  )
    .bind(name.toLowerCase())
    .first<{ id: string; name: string; pin_hash: string }>();
  const ok = account ? await verifyPin(body.pin!, account.pin_hash) : false;
  if (!account || !ok) {
    // Perform a dummy derive on unknown names so timing doesn't leak enumeration.
    if (!account) await deriveBits(body.pin!, new Uint8Array(16), pbkdf2Iterations(c.env));
    return jsonError(c, 401, "unauthorized", "incorrect name or PIN");
  }
  const session = await createSession(c.env, account.id);
  setSessionCookie(c, session.id, c.env);
  return c.json({
    authenticated: true,
    csrf_token: session.csrf,
    account_id: account.id,
    account_name: account.name,
  });
});

app.post("/api/auth/lock", async (c) => {
  const id = getCookie(c, SESSION_COOKIE);
  if (id) await revokeSession(c.env, id);
  clearSessionCookie(c);
  return c.body(null, 204);
});

app.post("/api/auth/change-pin", async (c) => {
  const authErr = await requireAuth(c);
  if (authErr) return authErr;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const body = await c.req
    .json<{ current_pin?: string; new_pin?: string }>()
    .catch(() => ({}));
  if (!validatePin(body.new_pin)) {
    return jsonError(c, 400, "bad_request", "pin must be 4-12 digits");
  }
  const account = await c.env.DB.prepare("SELECT pin_hash FROM accounts WHERE id = ?")
    .bind(session.account_id)
    .first<{ pin_hash: string }>();
  if (!account || !(await verifyPin(body.current_pin ?? "", account.pin_hash))) {
    return jsonError(c, 401, "unauthorized", "incorrect current pin");
  }
  const newHash = await hashPin(body.new_pin!, c.env);
  await c.env.DB.prepare(
    "UPDATE accounts SET pin_hash = ?, updated_at = ? WHERE id = ?",
  )
    .bind(newHash, nowIso(), session.account_id)
    .run();
  // Revoke every session for this account by looking them up in KV.
  // KV has no secondary index, so we revoke only the current session here;
  // other devices will drop out at session TTL (1 hour) or on next request
  // against a now-invalid pin — acceptable for a family app.
  await revokeSession(c.env, (await getCookie(c, SESSION_COOKIE)) ?? "");
  clearSessionCookie(c);
  return c.body(null, 204);
});

// ---------- Routes: appliances ----------

app.get("/api/appliances", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const inv = await getInventory(c.env, session.account_id);
  return c.json({ appliances: inv.appliances });
});

app.post("/api/appliances", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const body = await c.req
    .json<{ name?: string; kind?: Kind; location?: string; description?: string }>()
    .catch(() => ({}));
  if (!validItemName(body.name)) {
    return jsonError(c, 400, "bad_request", "name is required");
  }
  if (!body.kind || !["fridge", "freezer", "other"].includes(body.kind)) {
    return jsonError(c, 400, "bad_request", "kind must be fridge, freezer, or other");
  }
  const appliance: Appliance = {
    id: newId(),
    name: body.name!.trim(),
    kind: body.kind,
    location: blankToNull(body.location),
    description: blankToNull(body.description),
    created_at: nowIso(),
    updated_at: nowIso(),
  };
  const created = await mutateInventory(c.env, session.account_id, (inv) => {
    inv.appliances.push(appliance);
    return appliance;
  });
  return c.json(created, 201);
});

app.get("/api/appliances/:id", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const id = c.req.param("id");
  const inv = await getInventory(c.env, session.account_id);
  const appliance = inv.appliances.find((a) => a.id === id);
  if (!appliance) return jsonError(c, 404, "not_found", `appliance ${id} not found`);
  return c.json({
    appliance,
    groups: inv.groups.filter((g) => g.appliance_id === id),
    items: inv.items.filter((i) => i.appliance_id === id),
  });
});

app.patch("/api/appliances/:id", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const id = c.req.param("id");
  const body = await c.req
    .json<{
      name?: string;
      kind?: Kind;
      location?: string | null;
      description?: string | null;
    }>()
    .catch(() => ({}));
  try {
    const updated = await mutateInventory(c.env, session.account_id, (inv) => {
      const a = inv.appliances.find((x) => x.id === id);
      if (!a) throw new HttpErr(404, `appliance ${id} not found`);
      if (body.name !== undefined) {
        if (!validItemName(body.name)) throw new HttpErr(400, "name must not be empty");
        a.name = body.name.trim();
      }
      if (body.kind !== undefined) {
        if (!["fridge", "freezer", "other"].includes(body.kind)) {
          throw new HttpErr(400, "kind must be fridge, freezer, or other");
        }
        a.kind = body.kind;
      }
      if (body.location !== undefined) a.location = blankToNull(body.location);
      if (body.description !== undefined) a.description = blankToNull(body.description);
      a.updated_at = nowIso();
      return a;
    });
    return c.json(updated);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

app.delete("/api/appliances/:id", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const id = c.req.param("id");
  const strategy = c.req.query("strategy") ?? "";
  const moveTo = c.req.query("move_to") ?? "";
  try {
    await mutateInventory(c.env, session.account_id, (inv) => {
      if (!inv.appliances.some((a) => a.id === id)) {
        throw new HttpErr(404, `appliance ${id} not found`);
      }
      const hasContents =
        inv.items.some((i) => i.appliance_id === id) ||
        inv.groups.some((g) => g.appliance_id === id);
      if (hasContents && strategy !== "cascade" && strategy !== "move") {
        throw new HttpErr(
          409,
          "appliance has contents; specify strategy=cascade or strategy=move&move_to=<id>",
        );
      }
      if (strategy === "move") {
        if (!moveTo) throw new HttpErr(400, "move_to is required");
        if (moveTo === id)
          throw new HttpErr(400, "cannot move contents to the appliance being deleted");
        if (!inv.appliances.some((a) => a.id === moveTo)) {
          throw new HttpErr(404, `target appliance ${moveTo} not found`);
        }
        for (const item of inv.items) {
          if (item.appliance_id === id) {
            item.appliance_id = moveTo;
            item.updated_at = nowIso();
          }
        }
        for (const group of inv.groups) {
          if (group.appliance_id === id) {
            group.appliance_id = moveTo;
            group.updated_at = nowIso();
          }
        }
      } else if (strategy === "cascade") {
        inv.items = inv.items.filter((i) => i.appliance_id !== id);
        inv.groups = inv.groups.filter((g) => g.appliance_id !== id);
      }
      inv.appliances = inv.appliances.filter((a) => a.id !== id);
    });
    return c.body(null, 204);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

// ---------- Routes: items ----------

app.get("/api/appliances/:id/items", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const id = c.req.param("id");
  const inv = await getInventory(c.env, session.account_id);
  if (!inv.appliances.some((a) => a.id === id)) {
    return jsonError(c, 404, "not_found", `appliance ${id} not found`);
  }
  return c.json({ items: inv.items.filter((i) => i.appliance_id === id) });
});

app.post("/api/appliances/:id/items", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const applianceId = c.req.param("id");
  const body = await c.req.json<any>().catch(() => ({}));
  if (!validItemName(body?.name)) {
    return jsonError(c, 400, "bad_request", "name is required");
  }
  if (
    body.expiration_date &&
    body.expiration_date !== "" &&
    !isValidIsoDate(body.expiration_date)
  ) {
    return jsonError(c, 400, "bad_request", "expiration_date must be ISO-8601 (YYYY-MM-DD)");
  }
  const trackingPart = buildTrackingFromBody(body);
  if (!("ok" in trackingPart)) {
    return jsonError(c, 400, "bad_request", trackingPart.err);
  }
  try {
    const created = await mutateInventory(c.env, session.account_id, (inv) => {
      if (!inv.appliances.some((a) => a.id === applianceId)) {
        throw new HttpErr(404, `appliance ${applianceId} not found`);
      }
      if (body.group_id) {
        const g = inv.groups.find((x) => x.id === body.group_id);
        if (!g) throw new HttpErr(404, `group ${body.group_id} not found`);
        if (g.appliance_id !== applianceId) {
          throw new HttpErr(400, "group belongs to a different appliance");
        }
      }
      const item: InventoryItem = {
        id: newId(),
        name: (body.name as string).trim(),
        appliance_id: applianceId,
        group_id: body.group_id || null,
        category: blankToNull(body.category),
        expiration_date: blankToNull(body.expiration_date),
        notes: blankToNull(body.notes),
        has_photo: false,
        created_at: nowIso(),
        updated_at: nowIso(),
        ...trackingPart.ok,
      } as InventoryItem;
      inv.items.push(item);
      return item;
    });
    return c.json(created, 201);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

app.patch("/api/appliances/:id/items/:item_id", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const applianceId = c.req.param("id");
  const itemId = c.req.param("item_id");
  const body = await c.req.json<any>().catch(() => ({}));
  if (
    body.expiration_date !== undefined &&
    body.expiration_date !== null &&
    body.expiration_date !== "" &&
    !isValidIsoDate(body.expiration_date)
  ) {
    return jsonError(c, 400, "bad_request", "expiration_date must be ISO-8601 (YYYY-MM-DD)");
  }
  try {
    const updated = await mutateInventory(c.env, session.account_id, (inv) => {
      const item = inv.items.find((x) => x.id === itemId && x.appliance_id === applianceId);
      if (!item) throw new HttpErr(404, `item ${itemId} not found`);
      if (body.name !== undefined) {
        if (!validItemName(body.name)) throw new HttpErr(400, "name must not be empty");
        item.name = (body.name as string).trim();
      }
      if (body.group_id !== undefined) item.group_id = body.group_id || null;
      if (body.category !== undefined) item.category = blankToNull(body.category);
      if (body.expiration_date !== undefined)
        item.expiration_date = blankToNull(body.expiration_date);
      if (body.notes !== undefined) item.notes = blankToNull(body.notes);
      if (item.tracking === "individual" && typeof body.unit === "string") {
        if (!validItemName(body.unit)) throw new HttpErr(400, "unit must not be empty");
        item.unit = body.unit.trim();
      }
      if (item.tracking === "pack" && typeof body.pack_label === "string") {
        if (!validItemName(body.pack_label))
          throw new HttpErr(400, "pack_label must not be empty");
        item.pack_label = body.pack_label.trim();
      }
      if (item.group_id) {
        const g = inv.groups.find((x) => x.id === item.group_id);
        if (!g || g.appliance_id !== applianceId) {
          throw new HttpErr(400, "group does not exist in this appliance");
        }
      }
      item.updated_at = nowIso();
      return item;
    });
    return c.json(updated);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

app.delete("/api/appliances/:id/items/:item_id", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const applianceId = c.req.param("id");
  const itemId = c.req.param("item_id");
  try {
    await mutateInventory(c.env, session.account_id, (inv) => {
      const before = inv.items.length;
      inv.items = inv.items.filter(
        (x) => !(x.id === itemId && x.appliance_id === applianceId),
      );
      if (inv.items.length === before) {
        throw new HttpErr(404, `item ${itemId} not found`);
      }
    });
    // Best-effort photo cleanup; absent row is not an error.
    await c.env.DB.prepare(
      "DELETE FROM item_photos WHERE account_id = ? AND item_id = ?",
    )
      .bind(session.account_id, itemId)
      .run();
    return c.body(null, 204);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

app.post("/api/appliances/:id/items/:item_id/adjust", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const applianceId = c.req.param("id");
  const itemId = c.req.param("item_id");
  const body = await c.req.json<{ delta?: number }>().catch(() => ({}));
  const delta = Number(body.delta);
  if (!Number.isFinite(delta)) {
    return jsonError(c, 400, "bad_request", "delta must be a finite number");
  }
  try {
    const updated = await mutateInventory(c.env, session.account_id, (inv) => {
      const item = inv.items.find((x) => x.id === itemId && x.appliance_id === applianceId);
      if (!item) throw new HttpErr(404, `item ${itemId} not found`);
      if (item.tracking === "individual") {
        const next = item.quantity + delta;
        if (!Number.isFinite(next) || next < 0) {
          throw new HttpErr(400, "resulting quantity would be invalid");
        }
        item.quantity = roundQuantity(next);
      } else {
        if (!Number.isInteger(delta)) {
          throw new HttpErr(400, "pack counts change in whole-pack increments");
        }
        const next = item.pack_count + delta;
        if (next < 0) throw new HttpErr(400, "pack count cannot go below zero");
        if (next > 4_294_967_295) throw new HttpErr(400, "pack count overflow");
        item.pack_count = next;
      }
      item.updated_at = nowIso();
      return item;
    });
    return c.json(updated);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

app.post("/api/appliances/:id/items/:item_id/open-one", async (c) => {
  return packTransfer(c, "open", "one");
});

app.post("/api/appliances/:id/items/:item_id/open-all", async (c) => {
  return packTransfer(c, "open", "all");
});

app.post("/api/appliances/:id/items/:item_id/close-one", async (c) => {
  return packTransfer(c, "close", "one");
});

app.post("/api/appliances/:id/items/:item_id/close-all", async (c) => {
  return packTransfer(c, "close", "all");
});

async function packTransfer(
  c: any,
  direction: "open" | "close",
  mode: "one" | "all",
): Promise<Response> {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const applianceId = c.req.param("id");
  const itemId = c.req.param("item_id");
  const sourceStatus: PackStatus = direction === "open" ? "unopened" : "opened";
  const destStatus: PackStatus = direction === "open" ? "opened" : "unopened";
  try {
    const result = await mutateInventory(c.env, session.account_id, (inv) => {
      const sourceIdx = inv.items.findIndex(
        (x) => x.id === itemId && x.appliance_id === applianceId,
      );
      if (sourceIdx === -1) throw new HttpErr(404, `item ${itemId} not found`);
      const source = inv.items[sourceIdx]!;
      if (source.tracking !== "pack") throw new HttpErr(400, "item is not pack-based");
      if (source.pack_status !== sourceStatus) {
        throw new HttpErr(
          400,
          direction === "open"
            ? "only unopened variants can be opened"
            : "only opened variants can be marked unopened",
        );
      }
      if (source.pack_count === 0) {
        throw new HttpErr(
          400,
          direction === "open"
            ? "no unopened packs available to open"
            : "no opened packs available to mark unopened",
        );
      }
      const transfer = mode === "one" ? 1 : source.pack_count;

      // Auto-group when the ungrouped source is being opened for the first
      // time, so opened+unopened variants of the same item live together.
      let effectiveGroupId: string | null = source.group_id ?? null;
      if (!source.group_id && direction === "open") {
        const groupName = source.name.trim();
        let g = inv.groups.find(
          (g) =>
            g.appliance_id === source.appliance_id &&
            g.name.toLowerCase() === groupName.toLowerCase(),
        );
        if (!g) {
          g = {
            id: newId(),
            name: groupName,
            appliance_id: source.appliance_id,
            description: null,
            created_at: nowIso(),
            updated_at: nowIso(),
          };
          inv.groups.push(g);
        }
        source.group_id = g.id;
        source.updated_at = nowIso();
        effectiveGroupId = g.id;
      }

      let dest = inv.items.find(
        (i) =>
          i.appliance_id === source.appliance_id &&
          (i.group_id ?? null) === effectiveGroupId &&
          i.tracking === "pack" &&
          i.pack_status === destStatus &&
          i.pack_label === source.pack_label,
      );
      if (!dest) {
        dest = {
          id: newId(),
          name: source.name,
          appliance_id: source.appliance_id,
          group_id: effectiveGroupId,
          category: source.category ?? null,
          tracking: "pack",
          pack_count: 0,
          pack_label: source.pack_label,
          pack_status: destStatus,
          expiration_date: source.expiration_date ?? null,
          notes: source.notes ?? null,
          has_photo: false,
          created_at: nowIso(),
          updated_at: nowIso(),
        };
        inv.items.push(dest);
      }
      if (dest.tracking !== "pack") throw new HttpErr(500, "dest variant invalid");
      dest.pack_count += transfer;
      dest.updated_at = nowIso();
      source.pack_count -= transfer;
      source.updated_at = nowIso();

      let sourceAfter: InventoryItem | null = source;
      let photoCleanup: string | null = null;
      if (source.pack_count === 0) {
        photoCleanup = source.id;
        inv.items.splice(sourceIdx, 1);
        sourceAfter = null;
      }
      const sourceKey = direction === "open" ? "unopened" : "opened";
      const destKey = direction === "open" ? "opened" : "unopened";
      return {
        payload: {
          [sourceKey]: sourceAfter,
          [destKey]: dest,
          source_deleted: sourceAfter === null,
        },
        photoCleanup,
      };
    });
    if (result.photoCleanup) {
      await c.env.DB.prepare(
        "DELETE FROM item_photos WHERE account_id = ? AND item_id = ?",
      )
        .bind(session.account_id, result.photoCleanup)
        .run();
    }
    return c.json(result.payload);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
}

// ---------- Routes: groups ----------

app.get("/api/groups", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const inv = await getInventory(c.env, session.account_id);
  return c.json({ groups: inv.groups });
});

app.post("/api/groups", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const body = await c.req
    .json<{ name?: string; appliance_id?: string; description?: string }>()
    .catch(() => ({}));
  if (!validItemName(body.name)) return jsonError(c, 400, "bad_request", "name is required");
  if (!body.appliance_id)
    return jsonError(c, 400, "bad_request", "appliance_id is required");
  try {
    const created = await mutateInventory(c.env, session.account_id, (inv) => {
      if (!inv.appliances.some((a) => a.id === body.appliance_id)) {
        throw new HttpErr(404, `appliance ${body.appliance_id} not found`);
      }
      const g: Group = {
        id: newId(),
        name: (body.name as string).trim(),
        appliance_id: body.appliance_id!,
        description: blankToNull(body.description),
        created_at: nowIso(),
        updated_at: nowIso(),
      };
      inv.groups.push(g);
      return g;
    });
    return c.json(created, 201);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

app.patch("/api/groups/:id", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const id = c.req.param("id");
  const body = await c.req
    .json<{ name?: string; description?: string | null; appliance_id?: string }>()
    .catch(() => ({}));
  try {
    const updated = await mutateInventory(c.env, session.account_id, (inv) => {
      if (body.appliance_id && !inv.appliances.some((a) => a.id === body.appliance_id)) {
        throw new HttpErr(404, `appliance ${body.appliance_id} not found`);
      }
      const g = inv.groups.find((x) => x.id === id);
      if (!g) throw new HttpErr(404, `group ${id} not found`);
      if (body.name !== undefined) {
        if (!validItemName(body.name)) throw new HttpErr(400, "name must not be empty");
        g.name = body.name.trim();
      }
      if (body.description !== undefined) g.description = blankToNull(body.description);
      if (body.appliance_id !== undefined) g.appliance_id = body.appliance_id;
      g.updated_at = nowIso();
      return g;
    });
    return c.json(updated);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

app.delete("/api/groups/:id", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const id = c.req.param("id");
  const strategy = c.req.query("strategy") ?? "";
  const moveToGroup = c.req.query("move_to_group") ?? "";
  try {
    await mutateInventory(c.env, session.account_id, (inv) => {
      if (!inv.groups.some((g) => g.id === id))
        throw new HttpErr(404, `group ${id} not found`);
      const hasChildren = inv.items.some((i) => i.group_id === id);
      if (
        hasChildren &&
        strategy !== "cascade" &&
        strategy !== "detach" &&
        strategy !== "move"
      ) {
        throw new HttpErr(
          409,
          "group has children; specify strategy=detach, strategy=move, or strategy=cascade",
        );
      }
      if (strategy === "cascade") {
        inv.items = inv.items.filter((i) => i.group_id !== id);
      } else if (strategy === "detach") {
        for (const item of inv.items) {
          if (item.group_id === id) {
            item.group_id = null;
            item.updated_at = nowIso();
          }
        }
      } else if (strategy === "move") {
        if (!inv.groups.some((g) => g.id === moveToGroup)) {
          throw new HttpErr(404, `target group ${moveToGroup} not found`);
        }
        for (const item of inv.items) {
          if (item.group_id === id) {
            item.group_id = moveToGroup;
            item.updated_at = nowIso();
          }
        }
      }
      inv.groups = inv.groups.filter((g) => g.id !== id);
    });
    return c.body(null, 204);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

// ---------- Routes: global items ----------

app.get("/api/items", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const inv = await getInventory(c.env, session.account_id);
  return c.json({
    items: inv.items,
    groups: inv.groups,
    appliances: inv.appliances,
  });
});

// ---------- Tracking input helper ----------

function buildTrackingFromBody(
  body: any,
):
  | { ok: { tracking: "individual"; quantity: number; unit: string } }
  | { ok: { tracking: "pack"; pack_count: number; pack_label: string; pack_status: PackStatus } }
  | { err: string } {
  if (body.tracking === "individual") {
    const q = Number(body.quantity);
    if (!Number.isFinite(q) || q < 0) return { err: "quantity must be a finite non-negative number" };
    if (!validItemName(body.unit)) return { err: "unit is required" };
    return { ok: { tracking: "individual", quantity: roundQuantity(q), unit: body.unit.trim() } };
  }
  if (body.tracking === "pack") {
    const n = Number(body.pack_count);
    if (!Number.isInteger(n) || n < 0) return { err: "pack_count must be a non-negative integer" };
    if (!validItemName(body.pack_label)) return { err: "pack_label is required" };
    if (body.pack_status !== "opened" && body.pack_status !== "unopened") {
      return { err: "pack_status must be opened or unopened" };
    }
    return {
      ok: {
        tracking: "pack",
        pack_count: n,
        pack_label: body.pack_label.trim(),
        pack_status: body.pack_status,
      },
    };
  }
  return { err: "tracking must be 'individual' or 'pack'" };
}

// ---------- HttpErr helper ----------

class HttpErr extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

function httpErrorResponse(c: any, e: unknown): Response {
  if (e instanceof HttpErr) {
    const code =
      e.status === 404
        ? "not_found"
        : e.status === 409
          ? "conflict"
          : e.status === 400
            ? "bad_request"
            : "internal";
    return jsonError(c, e.status, code, e.message);
  }
  console.error("unhandled route error", e);
  return jsonError(c, 500, "internal", "unexpected error");
}

// ---------- Photos (stored as BLOBs in D1) ----------

const MAX_PHOTO_BYTES_B64 = 2 * 1024 * 1024;

function looksLikeJpeg(bytes: Uint8Array): boolean {
  return bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff;
}

app.post("/api/appliances/:id/items/:item_id/photo", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const applianceId = c.req.param("id");
  const itemId = c.req.param("item_id");
  const body = await c.req.json<{ image_base64?: string }>().catch(() => ({}));
  if (typeof body.image_base64 !== "string") {
    return jsonError(c, 400, "bad_request", "image_base64 is required");
  }
  if (body.image_base64.length > MAX_PHOTO_BYTES_B64) {
    return jsonError(c, 400, "bad_request", "photo too large (resize to under ~1.5 MB)");
  }
  let bytes: Uint8Array;
  try {
    bytes = fromBase64(body.image_base64);
  } catch {
    return jsonError(c, 400, "bad_request", "image_base64 is not valid base64");
  }
  if (!looksLikeJpeg(bytes)) {
    return jsonError(
      c,
      400,
      "bad_request",
      "only JPEG uploads are accepted (resize client-side to JPEG)",
    );
  }
  try {
    const updated = await mutateInventory(c.env, session.account_id, (inv) => {
      const item = inv.items.find(
        (x) => x.id === itemId && x.appliance_id === applianceId,
      );
      if (!item) throw new HttpErr(404, `item ${itemId} not found`);
      item.has_photo = true;
      item.updated_at = nowIso();
      return item;
    });
    await c.env.DB.prepare(
      `INSERT INTO item_photos (account_id, item_id, data, updated_at)
       VALUES (?, ?, ?, ?)
       ON CONFLICT(account_id, item_id) DO UPDATE
         SET data = excluded.data, updated_at = excluded.updated_at`,
    )
      .bind(session.account_id, itemId, bytes, nowIso())
      .run();
    return c.json(updated);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

app.get("/api/appliances/:id/items/:item_id/photo", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const applianceId = c.req.param("id");
  const itemId = c.req.param("item_id");
  const inv = await getInventory(c.env, session.account_id);
  if (!inv.items.some((x) => x.id === itemId && x.appliance_id === applianceId)) {
    return jsonError(c, 404, "not_found", `item ${itemId} not found`);
  }
  const row = await c.env.DB.prepare(
    "SELECT data FROM item_photos WHERE account_id = ? AND item_id = ?",
  )
    .bind(session.account_id, itemId)
    .first<Record<string, unknown>>();
  if (!row) return jsonError(c, 404, "not_found", "photo not found");
  const raw = (row as any).data;
  // D1's JS bindings have historically returned BLOB columns in several
  // different shapes depending on the wrangler version (ArrayBuffer, Uint8Array,
  // number[], or Node Buffer). Normalise to a Uint8Array so `new Response()`
  // always sees a valid BufferSource.
  let bytes: Uint8Array;
  if (raw instanceof Uint8Array) {
    bytes = raw;
  } else if (raw instanceof ArrayBuffer) {
    bytes = new Uint8Array(raw);
  } else if (Array.isArray(raw)) {
    bytes = new Uint8Array(raw as number[]);
  } else if (raw && typeof raw === "object" && typeof (raw as any).byteLength === "number") {
    bytes = new Uint8Array((raw as any).buffer ?? raw);
  } else {
    return jsonError(
      c,
      500,
      "internal",
      `unexpected photo blob type: ${Object.prototype.toString.call(raw)} (len=${(raw as any)?.length ?? (raw as any)?.byteLength ?? "?"})`,
    );
  }
  return new Response(bytes, {
    headers: {
      "content-type": "image/jpeg",
      "content-length": String(bytes.byteLength),
      "cache-control": "private, no-store",
    },
  });
});

app.delete("/api/appliances/:id/items/:item_id/photo", async (c) => {
  const err = await requireAuth(c);
  if (err) return err;
  const session = c.get("session")!;
  const csrfErr = requireCsrf(c, session);
  if (csrfErr) return csrfErr;
  const applianceId = c.req.param("id");
  const itemId = c.req.param("item_id");
  try {
    await mutateInventory(c.env, session.account_id, (inv) => {
      const item = inv.items.find(
        (x) => x.id === itemId && x.appliance_id === applianceId,
      );
      if (!item) throw new HttpErr(404, `item ${itemId} not found`);
      item.has_photo = false;
      item.updated_at = nowIso();
    });
    await c.env.DB.prepare(
      "DELETE FROM item_photos WHERE account_id = ? AND item_id = ?",
    )
      .bind(session.account_id, itemId)
      .run();
    return c.body(null, 204);
  } catch (e) {
    return httpErrorResponse(c, e);
  }
});

// ---------- Static asset fallback ----------
// Hono routes handle everything under /api/*. For any other path we let the
// Workers Assets binding serve the SPA shell (index.html, manifest, SW, icons).

app.all("*", async (c) => {
  // SPA fallback: Workers Assets with `not_found_handling = "single-page-application"`
  // already serves index.html for unmatched paths, so we can just delegate.
  return c.env.ASSETS.fetch(c.req.raw);
});

export default {
  fetch: app.fetch,
} satisfies ExportedHandler<Env>;

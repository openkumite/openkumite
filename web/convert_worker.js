// The game data converted in this browser (index.html's Game data): kumite_convert_web in wasm (pkg/, web/build.sh),
// one thread, with the functions it imports: the picked files (FileReaderSync on slices, 1 MB read ahead), the packs in
// the site's storage (OPFS, through sync access handles: store 2k the pack, 2k + 1 its index, for the base, Tekken 2,
// Counter-Strike and the extras; store 8 the scratch file an archive is unpacked into while a job converts; stores 9
// and 10 the arcade stages' pack; 11 and 12 Tekken Tag Tournament's) and the log. Messages: {job: 'state' | 'convert' | 'remove' | 'install', part?,
// files?: [[path, File]] (a disc image's files by name, the Half-Life folder's by path), now?}; replies {log} as it
// goes, then {done: state} or {error, quota}.
const ready = import('./pkg/kumite_convert_web.js').then(async (m) => { await m.default(); return m; });
const STORES = ['assets.pack', 'assets.idx', 't2.pack', 't2.idx', 'cs.pack', 'cs.idx', 'extras.pack', 'extras.idx', 'unpacked.tmp', 'arcade.pack', 'arcade.idx', 'ttt.pack', 'ttt.idx'];
// a part's pack's name (Tekken 3's is the base; the arcade stages' is a pack of its own, not installed)
const PACK = { t3: 'assets', t3arcade: 'arcade' };

let files = [];   // the picked files, by id
let handles = []; // the stores, by number: a sync access handle, or a File (a store the job only reads)
const reader = new FileReaderSync();
let ahead = { id: -1, at: 0, bytes: new Uint8Array(0) };
// (Rust gets "Name: message" strings: a QuotaExceededError keeps its name)
const message = (e) => (typeof e === 'string' ? e : `${e?.name}: ${e?.message}`);
const wrap = (f) => (...a) => { try { return f(...a); } catch (e) { throw message(e); } };
Object.assign(self, {
  okc_input_name: (id) => files[id].name,
  okc_input_size: (id) => files[id].size,
  okc_input_read: wrap((id, at, buf) => {
    const end = at + buf.length;
    if (ahead.id !== id || at < ahead.at || end > ahead.at + ahead.bytes.length) {
      const slice = files[id].slice(at, Math.max(end, at + (1 << 20)));
      ahead = { id, at, bytes: new Uint8Array(reader.readAsArrayBuffer(slice)) };
      if (end > at + ahead.bytes.length) throw `${files[id].name} ends before byte ${end}`;
    }
    buf.set(ahead.bytes.subarray(at - ahead.at, end - ahead.at));
  }),
  okc_store_read: wrap((s, at, buf) => {
    const h = handles[s];
    if (h instanceof File) {
      const bytes = new Uint8Array(reader.readAsArrayBuffer(h.slice(at, at + buf.length)));
      if (bytes.length !== buf.length) throw `the pack ends before byte ${at + buf.length}`;
      return buf.set(bytes);
    }
    if (h.read(buf, { at }) !== buf.length) throw `the pack ends before byte ${at + buf.length}`;
  }),
  okc_store_write: wrap((s, at, data) => {
    if (handles[s] instanceof File) throw 'this job only reads that pack';
    if (handles[s].write(data, { at }) !== data.length) throw 'QuotaExceededError: a write was cut short';
  }),
  okc_store_size: wrap((s) => (handles[s] instanceof File ? handles[s].size : handles[s].getSize())),
  okc_store_truncate: wrap((s, len) => {
    // (a pack only read keeps what lies past its index: no part of it)
    if (!(handles[s] instanceof File)) return handles[s].truncate(len);
  }),
  okc_store_flush: wrap((s) => (handles[s] instanceof File ? undefined : handles[s].flush())),
  okc_log: (line) => postMessage({ log: line }),
  okc_panic: (m) => postMessage({ panic: m }),
});

onmessage = async ({ data }) => {
  const root = await navigator.storage.getDirectory();
  try {
    const { run, state } = await ready;
    // the packs this job writes (locked while it runs); the others it reads from a snapshot, which leaves them to
    // the game (a Tekken 2 job beside the game playing on Tekken 3's)
    const writes = data.job === 'state' ? [] : ['extras', 'unpacked', ...(data.part ? [PACK[data.part] ?? data.part] : [])];
    for (const name of STORES) {
      const file = await root.getFileHandle(name, { create: true });
      if (!writes.includes(name.split('.')[0])) { handles.push(await file.getFile()); continue; }
      // (a worker of an earlier page, or one terminated, lets go of its handle a moment later)
      for (let tries = 0; ; tries++) {
        try { handles.push(await file.createSyncAccessHandle()); break; } catch (e) {
          if (e?.name !== 'NoModificationAllowedError' || tries >= 50) throw e;
          await new Promise((ok) => setTimeout(ok, 100));
        }
      }
    }
    if (data.job === 'state') return postMessage({ done: JSON.parse(state()) });
    files = [];
    const id = (f) => files.push(f) - 1;
    const picked = (data.files ?? []).map(([path, f]) => [path, id(f)]);
    const options = { job: data.job, part: data.part, files: picked, now: data.now };
    postMessage({ done: JSON.parse(run(JSON.stringify(options))) });
  } catch (e) {
    const error = message(e);
    postMessage({ error, quota: /QuotaExceededError/.test(error) });
  } finally {
    for (const h of handles) if (!(h instanceof File)) h.close();
    handles = [];
  }
};

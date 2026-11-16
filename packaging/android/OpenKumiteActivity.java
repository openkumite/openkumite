package io.github.openkumite;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.ContentResolver;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.DocumentsContract;
import android.provider.DocumentsContract.Document;
import android.provider.OpenableColumns;
import android.view.View;
import java.io.File;
import java.io.FileOutputStream;
import java.io.FilterInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/**
 * The game's activity: Android's NativeActivity (the game is libopenkumite.so: lib.rs android_main) with what native
 * code cannot do. The system's pickers for the player's game files (a disc image's folder, the Half-Life folder, a
 * folder kumite_converter wrote, or a .zip of any), copied on a thread of their own into the app's files folder as
 * assets.part (android.rs reads the progress below and converts the copy, or checks and keeps a converted one); and
 * the system bars hidden (immersive full screen).
 */
public class OpenKumiteActivity extends NativeActivity {
    /** The copy, read by android.rs: 0 none, 1 copying, 2 done, 3 failed (importError says why). */
    public volatile int importState;
    /** Bytes copied of all (a .zip: of the .zip read). */
    public volatile long importDone, importTotal;
    public volatile String importError;

    /** What pick() picks: a folder kumite_converter wrote (manifest.json), a .zip, the folder of a disc image (its
     *  files: DISC_FILES), the Half-Life folder (its levels and textures: .bsp, .wad). */
    public static final int CONVERTED = 0, ZIP = 1, DISC = 2, GAME = 3;
    /** A disc image's files, in every format the converters read (ok_converter_common's disc module). */
    private static final String[] DISC_FILES = {".cue", ".bin", ".iso", ".chd", ".ccd", ".img", ".mds", ".mdf", ".ecm", ".pbp", ".wav",
        ".zip", ".7z", ".tar", ".gz", ".tgz", ".xz", ".txz", ".zst", ".tzst"};
    private static final int PICK = 0x4F4B;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        immersive();
    }

    @Override
    public void onWindowFocusChanged(boolean focus) {
        super.onWindowFocusChanged(focus);
        if (focus) immersive();
    }

    /** No status or navigation bar (a swipe from the edge shows them a moment). */
    @SuppressWarnings("deprecation")
    private void immersive() {
        getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
            | View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
    }

    private volatile int picking;

    /** From the game (any thread): the system's folder picker, or its file picker for a .zip (`what`: CONVERTED ...). */
    public void pick(final int what) {
        runOnUiThread(() -> {
            picking = what;
            Intent i = what == ZIP ? new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("application/zip")
                : new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
            try {
                startActivityForResult(i, PICK);
            } catch (ActivityNotFoundException e) {
                fail("This device has no file picker.");
            }
        });
    }

    /** From the game: the copy's outcome seen. */
    public void importSeen() {
        importState = 0;
    }

    @Override
    protected void onActivityResult(final int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != PICK || result != RESULT_OK || data == null || data.getData() == null || importState == 1) return;
        final int what = picking;
        final Uri uri = data.getData();
        importDone = 0;
        importTotal = 0;
        importError = null;
        importState = 1;
        new Thread(() -> {
            File out = new File(getExternalFilesDir(null), "assets.part");
            try {
                delete(out);
                if (what == ZIP) unzip(uri, out); else copyTree(uri, out, what);
                importState = 2;
            } catch (Exception e) {
                delete(out);
                String m = e.getMessage();
                fail(m != null && (m.startsWith("That ") || m.startsWith("Not enough")) ? m : "The copy stopped: " + e + ". Try again.");
            }
        }, "import").start();
    }

    private static boolean isDiscFile(String lower) {
        for (String e : DISC_FILES) {
            if (lower.endsWith(e)) return true;
        }
        return false;
    }

    private void fail(String message) {
        importError = message;
        importState = 3;
    }

    /** The picked folder's files (hidden ones left out) copied into `out`: a converted game's below its shallowest
     *  manifest.json; a disc image's files (DISC_FILES) in the folder itself; the game's levels and textures. */
    private void copyTree(Uri tree, File out, int what) throws IOException {
        ContentResolver cr = getContentResolver();
        List<Object[]> all = new ArrayList<>();
        list(cr, tree, DocumentsContract.getTreeDocumentId(tree), "", all);
        String base = "";
        if (what == CONVERTED) {
            base = null;
            for (Object[] f : all) {
                String p = (String) f[1];
                if ((p.equals("manifest.json") || p.endsWith("/manifest.json")) && (base == null || p.length() - p.replace("/", "").length() < base.length() - base.replace("/", "").length())) {
                    base = p.substring(0, p.length() - "manifest.json".length());
                }
            }
            if (base == null) throw new IOException("That folder has no manifest.json, so it isn’t a converted game. Choose the folder kumite_converter wrote.");
        }
        List<Object[]> files = new ArrayList<>();
        long need = 0;
        for (Object[] f : all) {
            String p = ((String) f[1]).toLowerCase(java.util.Locale.ROOT);
            boolean wanted = what == DISC ? !p.contains("/") && isDiscFile(p)
                : what == GAME ? p.endsWith(".bsp") || p.endsWith(".wad") : true;
            if (wanted && ((String) f[1]).startsWith(base)) {
                files.add(f);
                need += (Long) f[2];
            }
        }
        if (files.isEmpty()) {
            throw new IOException(what == DISC ? "That folder has no disc image in it (.cue, .bin, .chd, .iso, .ccd, .mds, .ecm, .pbp, or a .zip or .7z of it). Choose the folder that holds the disc image's files."
                : "That folder has no levels (.bsp). Choose the Half-Life folder, with cstrike/ and valve/.");
        }
        File dir = out.getParentFile();
        if (dir.getUsableSpace() < need) {
            throw new IOException(String.format("Not enough space: the game needs %.1f GB and this device has %.1f GB free. Free some space, then try again.", need / 1e9, dir.getUsableSpace() / 1e9));
        }
        importTotal = need;
        byte[] buf = new byte[1 << 20];
        for (Object[] f : files) {
            File to = new File(out, ((String) f[1]).substring(base.length()));
            to.getParentFile().mkdirs();
            try (InputStream in = cr.openInputStream(DocumentsContract.buildDocumentUriUsingTree(tree, (String) f[0]))) {
                if (in == null) throw new IOException("cannot read " + f[1]);
                copy(in, to, buf, true);
            }
        }
    }

    /** The files under document `doc` of `tree` (document id, path, size). */
    private void list(ContentResolver cr, Uri tree, String doc, String path, List<Object[]> out) throws IOException {
        String[] columns = { Document.COLUMN_DOCUMENT_ID, Document.COLUMN_DISPLAY_NAME, Document.COLUMN_MIME_TYPE, Document.COLUMN_SIZE };
        try (Cursor c = cr.query(DocumentsContract.buildChildDocumentsUriUsingTree(tree, doc), columns, null, null, null)) {
            if (c == null) throw new IOException("That folder could not be read.");
            while (c.moveToNext()) {
                String name = c.getString(1);
                if (name == null || name.startsWith(".") || name.contains("/")) continue;
                if (Document.MIME_TYPE_DIR.equals(c.getString(2))) list(cr, tree, c.getString(0), path + name + "/", out);
                else out.add(new Object[] { c.getString(0), path + name, c.getLong(3) });
            }
        }
    }

    /** A .zip's files into `out` (its folder inside, if any, is found by android.rs). */
    private void unzip(Uri zip, File out) throws IOException {
        ContentResolver cr = getContentResolver();
        try (Cursor c = cr.query(zip, new String[] { OpenableColumns.SIZE }, null, null, null)) {
            if (c != null && c.moveToFirst()) importTotal = c.getLong(0);
        }
        InputStream raw = cr.openInputStream(zip);
        if (raw == null) throw new IOException("That file could not be read.");
        // (the progress: the .zip's bytes read)
        InputStream counted = new FilterInputStream(raw) {
            @Override
            public int read(byte[] b, int off, int len) throws IOException {
                int n = super.read(b, off, len);
                if (n > 0) importDone += n;
                return n;
            }
        };
        String root = out.getCanonicalPath() + File.separator;
        byte[] buf = new byte[1 << 20];
        try (ZipInputStream z = new ZipInputStream(counted)) {
            for (ZipEntry e; (e = z.getNextEntry()) != null; ) {
                File to = new File(out, e.getName());
                // (an entry may not climb out of the folder)
                if (!to.getCanonicalPath().startsWith(root)) throw new IOException("a file outside the folder: " + e.getName());
                if (e.isDirectory()) {
                    to.mkdirs();
                    continue;
                }
                to.getParentFile().mkdirs();
                copy(z, to, buf, false);
            }
        }
    }

    private void copy(InputStream in, File to, byte[] buf, boolean count) throws IOException {
        try (OutputStream o = new FileOutputStream(to)) {
            for (int n; (n = in.read(buf)) > 0; ) {
                o.write(buf, 0, n);
                if (count) importDone += n;
            }
        }
    }

    private static void delete(File f) {
        File[] kids = f.listFiles();
        if (kids != null) for (File k : kids) delete(k);
        f.delete();
    }
}

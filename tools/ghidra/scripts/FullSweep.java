// Read-only function sweep.
// args: <outdir> <start_index> <count> <threads>
// Reads outdir/sweep-config.json for timeout_seconds and include_callees.

import generic.concurrent.GThreadPool;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.parallel.DecompileConfigurer;
import ghidra.app.decompiler.parallel.DecompilerCallback;
import ghidra.app.decompiler.parallel.ParallelDecompiler;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressOutOfBoundsException;
import ghidra.program.model.address.AddressSpace;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.listing.Program;
import ghidra.program.model.scalar.Scalar;
import ghidra.program.model.symbol.Namespace;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;
import ghidra.program.model.symbol.Symbol;
import ghidra.util.task.TaskMonitor;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.time.Instant;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.zip.GZIPOutputStream;

public class FullSweep extends GhidraScript {
    private static final int DEFAULT_TIMEOUT_SECONDS = 30;
    private static final int MAX_CALLERS = 50;
    private static final int MAX_CALLEES = 100;
    private static final int MAX_STRINGS = 50;
    private static final int MAX_STRING_CHARS = 200;
    private static final int MAX_C_CHARS = 200 * 1024;
    private static final int CHECKPOINT_EVERY = 1000;
    private static final int MAX_INSTRUCTION_WALK = 8000;
    private static final int MAX_VTABLE_REFS = 400;
    private static final int VTABLE_SCAN_STEPS = 0x40;
    private static final int POINTER_BYTES = 8;
    private static final int MIN_THREADS = 1;
    private static final int MAX_THREADS = 32;
    private static final int GZIP_BUFFER_BYTES = 1 << 16;
    private static final String THREAD_POOL_NAME = "Parallel Decompiler";
    private static final String CONFIG_FILE_NAME = "sweep-config.json";
    private static final String CHECKPOINT_FILE_NAME = "checkpoint.json";
    private static final String TYPEINFO_TOKEN = "typeinfo";
    private static final Pattern TIMEOUT_KEY = Pattern.compile("\"timeout_seconds\"\\s*:\\s*(\\d+)");
    private static final Pattern CALLEES_KEY = Pattern.compile("\"include_callees\"\\s*:\\s*(true|false)");

    private static final class Meta {
        int index;
        String entry = "";
        String name = "";
        String namespace = "";
        String signature = "";
        String callingConvention = "";
        long size;
        boolean thunk;
        String thunkTarget;
        String thunkTargetName;
        boolean external;
        List<String> callers = new ArrayList<String>();
        List<String> callees = new ArrayList<String>();
        List<String> strings = new ArrayList<String>();
        List<String> rtti = new ArrayList<String>();
        List<VtableHit> vtables = new ArrayList<VtableHit>();
    }

    private static final class VtableHit {
        String slotAddress;
        String vtableAddress;
        String symbol;
        String namespace;
        long slotIndex;

        VtableHit(String slotAddress, String vtableAddress, String symbol, String namespace, long slotIndex) {
            this.slotAddress = slotAddress;
            this.vtableAddress = vtableAddress;
            this.symbol = symbol;
            this.namespace = namespace;
            this.slotIndex = slotIndex;
        }
    }

    private static final class DecompOut {
        final String entry;
        final String cText;
        final String error;
        final boolean truncated;

        DecompOut(String entry, String cText, String error, boolean truncated) {
            this.entry = entry;
            this.cText = cText;
            this.error = error;
            this.truncated = truncated;
        }
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 4) {
            println("usage: FullSweep <outdir> <start_index> <count> <threads>");
            return;
        }
        File outDir = new File(args[0]);
        int startIndex = Integer.parseInt(args[1]);
        int count = Integer.parseInt(args[2]);
        int threads = Integer.parseInt(args[3]);
        if (startIndex < 0 || count < 0) {
            throw new IllegalArgumentException("start_index and count must be >= 0");
        }
        if (threads < MIN_THREADS || threads > MAX_THREADS) {
            throw new IllegalArgumentException("threads must be " + MIN_THREADS + ".." + MAX_THREADS);
        }
        if (!outDir.isDirectory() && !outDir.mkdirs()) {
            throw new IOException("cannot create " + outDir.getAbsolutePath());
        }
        File funcDir = new File(outDir, "funcs");
        if (!funcDir.isDirectory() && !funcDir.mkdirs()) {
            throw new IOException("cannot create " + funcDir.getAbsolutePath());
        }

        int timeoutSeconds = DEFAULT_TIMEOUT_SECONDS;
        boolean includeCallees = true;
        File config = new File(outDir, CONFIG_FILE_NAME);
        if (config.isFile()) {
            String text = Files.readString(config.toPath());
            Matcher timeoutMatch = TIMEOUT_KEY.matcher(text);
            if (timeoutMatch.find()) {
                timeoutSeconds = Integer.parseInt(timeoutMatch.group(1));
            }
            Matcher calleeMatch = CALLEES_KEY.matcher(text);
            if (calleeMatch.find()) {
                includeCallees = Boolean.parseBoolean(calleeMatch.group(1));
            }
        }
        if (timeoutSeconds < 1) {
            timeoutSeconds = 1;
        }
        println("config timeout_seconds=" + timeoutSeconds + " include_callees=" + includeCallees);

        long selectStart = System.currentTimeMillis();
        List<Function> selected = new ArrayList<Function>();
        int total = 0;
        FunctionIterator iterator = currentProgram.getFunctionManager().getFunctions(true);
        while (iterator.hasNext()) {
            Function function = iterator.next();
            if (function == null) {
                continue;
            }
            if (total >= startIndex && selected.size() < count) {
                selected.add(function);
            }
            total++;
        }
        long selectMs = System.currentTimeMillis() - selectStart;
        println("selected " + selected.size() + " of total " + total + " in " + selectMs + " ms");
        if (selected.isEmpty()) {
            writeCheckpoint(outDir, startIndex, total);
            println("nothing to do");
            return;
        }

        GThreadPool pool = GThreadPool.getSharedThreadPool(THREAD_POOL_NAME);
        if (pool.getMinThreadCount() > threads) {
            pool.setMinThreadCount(threads);
        }
        pool.setMaxThreadCount(threads);
        println("decompiler_threads=" + pool.getMaxThreadCount());

        final int decompileTimeout = timeoutSeconds;
        DecompileConfigurer configurer = new DecompileConfigurer() {
            @Override
            public void configure(DecompInterface decompiler) {
                decompiler.toggleCCode(true);
                decompiler.toggleSyntaxTree(false);
            }
        };
        DecompilerCallback<DecompOut> callback = new DecompilerCallback<DecompOut>(currentProgram, configurer) {
            @Override
            public DecompOut process(DecompileResults results, TaskMonitor taskMonitor) {
                return decompileOne(results);
            }
        };
        callback.setTimeout(decompileTimeout);

        File batchFile = new File(funcDir, "batch-" + startIndex + ".jsonl.gz");
        boolean append = false;
        long metaMs = 0;
        long decompileMs = 0;
        int written = 0;
        try {
            for (int offset = 0; offset < selected.size(); offset += CHECKPOINT_EVERY) {
                monitor.checkCancelled();
                int end = Math.min(offset + CHECKPOINT_EVERY, selected.size());
                List<Function> chunk = selected.subList(offset, end);
                long metaStart = System.currentTimeMillis();
                Map<String, Meta> metas = new HashMap<String, Meta>();
                List<Meta> ordered = new ArrayList<Meta>();
                for (int i = 0; i < chunk.size(); i++) {
                    Meta meta = collectMeta(chunk.get(i), startIndex + offset + i, includeCallees);
                    metas.put(meta.entry, meta);
                    ordered.add(meta);
                }
                metaMs += System.currentTimeMillis() - metaStart;

                long decompileStart = System.currentTimeMillis();
                Map<String, DecompOut> decompiled = decompileChunk(callback, chunk);
                decompileMs += System.currentTimeMillis() - decompileStart;

                List<Function> missing = new ArrayList<Function>();
                for (Function function : chunk) {
                    String entry = hexAddress(function.getEntryPoint());
                    if (!decompiled.containsKey(entry)) {
                        missing.add(function);
                    }
                }
                if (!missing.isEmpty()) {
                    println("sequential fallback for " + missing.size() + " functions");
                    long fallbackStart = System.currentTimeMillis();
                    decompiled.putAll(decompileSequential(missing, decompileTimeout));
                    decompileMs += System.currentTimeMillis() - fallbackStart;
                }

                writeChunk(batchFile, append, ordered, decompiled);
                append = true;
                written += ordered.size();
                writeCheckpoint(outDir, startIndex + written, total);
                println("checkpoint next_index=" + (startIndex + written) + " total=" + total);
            }
        }
        finally {
            callback.dispose();
        }
        println("SWEEP_TIMING start=" + startIndex
            + " written=" + written
            + " total=" + total
            + " select_ms=" + selectMs
            + " meta_ms=" + metaMs
            + " decompile_ms=" + decompileMs);
    }

    private Map<String, DecompOut> decompileChunk(DecompilerCallback<DecompOut> callback, List<Function> chunk)
            throws Exception {
        Map<String, DecompOut> byEntry = new HashMap<String, DecompOut>();
        List<Function> work = new ArrayList<Function>();
        for (Function function : chunk) {
            if (function.isExternal()) {
                byEntry.put(hexAddress(function.getEntryPoint()),
                    new DecompOut(hexAddress(function.getEntryPoint()), null, "external function", false));
            }
            else {
                work.add(function);
            }
        }
        if (work.isEmpty()) {
            return byEntry;
        }
        try {
            List<DecompOut> outs = ParallelDecompiler.decompileFunctions(callback, work, monitor);
            for (DecompOut out : outs) {
                if (out != null && out.entry != null) {
                    byEntry.put(out.entry, out);
                }
            }
        }
        catch (Exception ex) {
            println("parallel decompile failed: " + ex.getClass().getSimpleName() + ": " + ex.getMessage());
        }
        return byEntry;
    }

    private Map<String, DecompOut> decompileSequential(List<Function> functions, int timeoutSeconds) {
        Map<String, DecompOut> byEntry = new HashMap<String, DecompOut>();
        DecompInterface decompiler = new DecompInterface();
        try {
            decompiler.toggleCCode(true);
            decompiler.toggleSyntaxTree(false);
            decompiler.openProgram(currentProgram);
            for (Function function : functions) {
                String entry = hexAddress(function.getEntryPoint());
                if (function.isExternal()) {
                    byEntry.put(entry, new DecompOut(entry, null, "external function", false));
                    continue;
                }
                try {
                    DecompileResults results = decompiler.decompileFunction(function, timeoutSeconds, monitor);
                    byEntry.put(entry, decompileOne(results));
                }
                catch (Exception ex) {
                    byEntry.put(entry, new DecompOut(entry, null, errorText(ex), false));
                }
            }
        }
        finally {
            decompiler.dispose();
        }
        return byEntry;
    }

    private DecompOut decompileOne(DecompileResults results) {
        if (results == null || results.getFunction() == null) {
            return new DecompOut("", null, "null decompile result", false);
        }
        String entry = hexAddress(results.getFunction().getEntryPoint());
        try {
            if (!results.decompileCompleted() || results.getDecompiledFunction() == null) {
                String message = results.getErrorMessage();
                if (message == null || message.isEmpty()) {
                    message = "decompile failed";
                }
                return new DecompOut(entry, null, message, false);
            }
            String text = results.getDecompiledFunction().getC();
            if (text == null || text.isEmpty()) {
                return new DecompOut(entry, null, "empty decompile", false);
            }
            boolean truncated = false;
            if (text.length() > MAX_C_CHARS) {
                text = text.substring(0, MAX_C_CHARS);
                truncated = true;
            }
            return new DecompOut(entry, text, null, truncated);
        }
        catch (Exception ex) {
            return new DecompOut(entry, null, errorText(ex), false);
        }
    }

    private Meta collectMeta(Function function, int index, boolean includeCallees) {
        Meta meta = new Meta();
        meta.index = index;
        meta.entry = hexAddress(function.getEntryPoint());
        meta.name = safe(function.getName());
        meta.namespace = namespaceOf(function);
        meta.thunk = function.isThunk();
        meta.external = function.isExternal();
        meta.size = bodySize(function);
        try {
            if (function.getSignature() != null) {
                meta.signature = safe(function.getSignature().getPrototypeString());
            }
        }
        catch (Exception ex) {
            meta.signature = "";
        }
        try {
            meta.callingConvention = safe(function.getCallingConventionName());
        }
        catch (Exception ex) {
            meta.callingConvention = "";
        }
        if (meta.thunk) {
            try {
                Function target = function.getThunkedFunction(false);
                if (target != null) {
                    meta.thunkTarget = hexAddress(target.getEntryPoint());
                    meta.thunkTargetName = safe(target.getName());
                }
            }
            catch (Exception ex) {
                meta.thunkTarget = null;
            }
        }
        collectCallers(function, meta);
        if (includeCallees) {
            collectCallees(function, meta);
        }
        collectStrings(function, meta);
        collectVtables(function, meta);
        return meta;
    }

    private void collectCallers(Function function, Meta meta) {
        try {
            Set<Function> callers = function.getCallingFunctions(monitor);
            List<String> addresses = new ArrayList<String>();
            if (callers != null) {
                for (Function caller : callers) {
                    if (caller != null && caller.getEntryPoint() != null) {
                        addresses.add(hexAddress(caller.getEntryPoint()));
                    }
                }
            }
            Collections.sort(addresses);
            if (addresses.size() > MAX_CALLERS) {
                addresses = addresses.subList(0, MAX_CALLERS);
            }
            meta.callers = addresses;
        }
        catch (Exception ex) {
            println("callers failed " + meta.entry + " " + ex.getMessage());
        }
    }

    private void collectCallees(Function function, Meta meta) {
        try {
            Set<Function> callees = function.getCalledFunctions(monitor);
            List<String> addresses = new ArrayList<String>();
            if (callees != null) {
                for (Function callee : callees) {
                    if (callee != null && callee.getEntryPoint() != null) {
                        addresses.add(hexAddress(callee.getEntryPoint()));
                    }
                }
            }
            Collections.sort(addresses);
            if (addresses.size() > MAX_CALLEES) {
                addresses = addresses.subList(0, MAX_CALLEES);
            }
            meta.callees = addresses;
        }
        catch (Exception ex) {
            println("callees failed " + meta.entry + " " + ex.getMessage());
        }
    }

    private void collectStrings(Function function, Meta meta) {
        if (function.getBody() == null || function.getBody().isEmpty()) {
            return;
        }
        LinkedHashSet<String> found = new LinkedHashSet<String>();
        InstructionIterator instructions = currentProgram.getListing().getInstructions(function.getBody(), true);
        int seen = 0;
        while (instructions.hasNext()) {
            Instruction instruction = instructions.next();
            seen++;
            if (seen > MAX_INSTRUCTION_WALK) {
                break;
            }
            Reference[] refs = instruction.getReferencesFrom();
            if (refs == null) {
                continue;
            }
            for (Reference ref : refs) {
                addString(ref.getToAddress(), found);
                if (found.size() >= MAX_STRINGS) {
                    break;
                }
            }
            if (found.size() >= MAX_STRINGS) {
                break;
            }
        }
        meta.strings.addAll(found);
    }

    private void addString(Address address, Set<String> found) {
        if (address == null || found.size() >= MAX_STRINGS) {
            return;
        }
        try {
            Data data = currentProgram.getListing().getDataContaining(address);
            if (data == null || !data.hasStringValue()) {
                return;
            }
            String text = null;
            Object value = data.getValue();
            if (value instanceof String) {
                text = (String) value;
            }
            else if (value != null) {
                text = String.valueOf(value);
            }
            if (text == null || text.isEmpty()) {
                return;
            }
            text = text.replace("\u0000", "");
            if (text.length() > MAX_STRING_CHARS) {
                text = text.substring(0, MAX_STRING_CHARS);
            }
            if (!text.isEmpty()) {
                found.add(text);
            }
        }
        catch (Exception ex) {
            // One bad data item must not stop the function.
        }
    }

    private void collectVtables(Function function, Meta meta) {
        Address entry = function.getEntryPoint();
        if (entry == null) {
            return;
        }
        ReferenceIterator refs = currentProgram.getReferenceManager().getReferencesTo(entry);
        int examined = 0;
        while (refs.hasNext() && examined < MAX_VTABLE_REFS && meta.vtables.size() < MAX_VTABLE_REFS) {
            Reference ref = refs.next();
            examined++;
            Address slot = ref.getFromAddress();
            if (slot == null) {
                continue;
            }
            if (currentProgram.getListing().getInstructionContaining(slot) != null) {
                continue;
            }
            scanVtable(slot, meta);
        }
    }

    private void scanVtable(Address slot, Meta meta) {
        Address cursor = slot;
        Program program = currentProgram;
        for (int step = 0; step < VTABLE_SCAN_STEPS; step++) {
            try {
                cursor = cursor.subtract(POINTER_BYTES);
            }
            catch (AddressOutOfBoundsException ex) {
                return;
            }
            try {
                Data data = program.getListing().getDataAt(cursor);
                if (data == null) {
                    continue;
                }
                Object value = data.getValue();
                if (!(value instanceof Scalar)) {
                    continue;
                }
                Scalar scalar = (Scalar) value;
                Address target = toAddr(scalar.getUnsignedValue());
                Symbol symbol = getSymbolAt(target);
                if (symbol == null || symbol.getName() == null) {
                    continue;
                }
                if (!symbol.getName().contains(TYPEINFO_TOKEN)) {
                    continue;
                }
                Address vtableStart = cursor.add(POINTER_BYTES);
                long slotIndex = -1;
                try {
                    slotIndex = slot.subtract(vtableStart) / POINTER_BYTES;
                }
                catch (Exception ex) {
                    slotIndex = -1;
                }
                String namespace = "";
                if (symbol.getParentNamespace() != null) {
                    namespace = safe(symbol.getParentNamespace().getName(true));
                }
                meta.vtables.add(new VtableHit(
                    hexAddress(slot),
                    hexAddress(vtableStart),
                    symbol.getName(),
                    namespace,
                    slotIndex));
                if (!meta.rtti.contains(symbol.getName())) {
                    meta.rtti.add(symbol.getName());
                }
                return;
            }
            catch (Exception ex) {
                // Keep scanning backward.
            }
        }
    }

    private static String namespaceOf(Function function) {
        try {
            Namespace parent = function.getParentNamespace();
            if (parent == null || parent.getName(true) == null) {
                return "";
            }
            return parent.getName(true);
        }
        catch (Exception ex) {
            return "";
        }
    }

    private static long bodySize(Function function) {
        try {
            if (function.getBody() == null) {
                return 0;
            }
            return function.getBody().getNumAddresses();
        }
        catch (Exception ex) {
            return 0;
        }
    }

    private void writeChunk(File batchFile, boolean append, List<Meta> ordered, Map<String, DecompOut> decompiled)
            throws IOException {
        OutputStream raw = new FileOutputStream(batchFile, append);
        GZIPOutputStream gzip = new GZIPOutputStream(raw, GZIP_BUFFER_BYTES);
        try {
            for (Meta meta : ordered) {
                DecompOut out = decompiled.get(meta.entry);
                if (out == null) {
                    out = new DecompOut(meta.entry, null, "missing decompile result", false);
                }
                String line = toJson(meta, out);
                gzip.write(line.getBytes(StandardCharsets.UTF_8));
                gzip.write('\n');
            }
            gzip.finish();
        }
        finally {
            gzip.close();
        }
    }

    private static String toJson(Meta meta, DecompOut out) {
        StringBuilder body = new StringBuilder(1024);
        body.append('{');
        field(body, "index", Integer.toString(meta.index), true);
        field(body, "entry", quote(meta.entry), false);
        field(body, "name", quote(meta.name), false);
        field(body, "namespace", quote(meta.namespace), false);
        field(body, "signature", quote(meta.signature), false);
        field(body, "calling_convention", quote(meta.callingConvention), false);
        field(body, "size", Long.toString(meta.size), false);
        field(body, "is_thunk", meta.thunk ? "true" : "false", false);
        field(body, "thunk_target", meta.thunkTarget == null ? "null" : quote(meta.thunkTarget), false);
        field(body, "thunk_target_name", meta.thunkTargetName == null ? "null" : quote(meta.thunkTargetName), false);
        field(body, "is_external", meta.external ? "true" : "false", false);
        field(body, "callers", stringArray(meta.callers), false);
        field(body, "callees", stringArray(meta.callees), false);
        field(body, "strings", stringArray(meta.strings), false);
        field(body, "rtti", stringArray(meta.rtti), false);
        field(body, "vtables", vtableArray(meta.vtables), false);
        if (out.error != null) {
            field(body, "error", quote(out.error), false);
        }
        else {
            field(body, "c", quote(out.cText == null ? "" : out.cText), false);
            if (out.truncated) {
                field(body, "c_truncated", "true", false);
            }
        }
        body.append('}');
        return body.toString();
    }

    private static void field(StringBuilder body, String name, String rawJson, boolean first) {
        if (!first) {
            body.append(',');
        }
        body.append('"').append(name).append("\":").append(rawJson);
    }

    private static String stringArray(List<String> values) {
        StringBuilder body = new StringBuilder();
        body.append('[');
        for (int i = 0; i < values.size(); i++) {
            if (i > 0) {
                body.append(',');
            }
            body.append(quote(values.get(i)));
        }
        body.append(']');
        return body.toString();
    }

    private static String vtableArray(List<VtableHit> hits) {
        StringBuilder body = new StringBuilder();
        body.append('[');
        for (int i = 0; i < hits.size(); i++) {
            if (i > 0) {
                body.append(',');
            }
            VtableHit hit = hits.get(i);
            body.append('{');
            field(body, "slot", quote(hit.slotAddress), true);
            field(body, "vtable", quote(hit.vtableAddress), false);
            field(body, "symbol", quote(hit.symbol), false);
            field(body, "namespace", quote(hit.namespace), false);
            field(body, "slot_index", Long.toString(hit.slotIndex), false);
            body.append('}');
        }
        body.append(']');
        return body.toString();
    }

    private static String quote(String value) {
        if (value == null) {
            return "null";
        }
        StringBuilder body = new StringBuilder(value.length() + 2);
        body.append('"');
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            switch (ch) {
                case '"':
                    body.append("\\\"");
                    break;
                case '\\':
                    body.append("\\\\");
                    break;
                case '\n':
                    body.append("\\n");
                    break;
                case '\r':
                    body.append("\\r");
                    break;
                case '\t':
                    body.append("\\t");
                    break;
                default:
                    if (ch < 0x20) {
                        body.append(String.format("\\u%04x", (int) ch));
                    }
                    else {
                        body.append(ch);
                    }
                    break;
            }
        }
        body.append('"');
        return body.toString();
    }

    private static void writeCheckpoint(File outDir, int nextIndex, int total) throws IOException {
        String doneAt = DateTimeFormatter.ISO_INSTANT.format(Instant.now());
        String body = "{\"next_index\": " + nextIndex
            + ", \"total\": " + total
            + ", \"done_at\": \"" + doneAt + "\"}\n";
        File dest = new File(outDir, CHECKPOINT_FILE_NAME);
        File tmp = new File(outDir, CHECKPOINT_FILE_NAME + ".tmp");
        Files.writeString(tmp.toPath(), body, StandardCharsets.UTF_8);
        try {
            Files.move(tmp.toPath(), dest.toPath(),
                java.nio.file.StandardCopyOption.REPLACE_EXISTING,
                java.nio.file.StandardCopyOption.ATOMIC_MOVE);
        }
        catch (IOException ex) {
            Files.move(tmp.toPath(), dest.toPath(), java.nio.file.StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private String hexAddress(Address address) {
        if (address == null) {
            return "";
        }
        String hex = "0x" + Long.toHexString(address.getOffset());
        AddressSpace space = address.getAddressSpace();
        AddressSpace defaultSpace = currentProgram.getAddressFactory().getDefaultAddressSpace();
        if (space != null && !space.equals(defaultSpace)) {
            return space.getName() + ":" + hex;
        }
        return hex;
    }

    private static String safe(String value) {
        return value == null ? "" : value;
    }

    private static String errorText(Exception ex) {
        String message = ex.getMessage();
        if (message == null || message.isEmpty()) {
            return ex.getClass().getSimpleName();
        }
        return ex.getClass().getSimpleName() + ": " + message;
    }
}

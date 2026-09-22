// Ghidra Java script: find a literal string, list xrefs, decompile containing functions.
// args[0] = string needle, args[1] = optional output file
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;
import ghidra.util.task.ConsoleTaskMonitor;
import java.io.*;
import java.util.*;

public class FindAndDecompile extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String needle = args[0];
        byte[] needleBytes = needle.getBytes("ISO-8859-1");
        Memory mem = currentProgram.getMemory();
        StringBuilder out = new StringBuilder();
        Address found = null;
        for (MemoryBlock block : mem.getBlocks()) {
            if (!block.isInitialized()) continue;
            Address a = block.getStart();
            while (a != null) {
                a = mem.findBytes(a, block.getEnd(), needleBytes, null, true, monitor);
                if (a == null) break;
                out.append("string at ").append(a.toString()).append('\n');
                if (found == null) found = a;
                Address next = a.add(1);
                a = next.compareTo(block.getEnd()) > 0 ? null : next;
            }
        }
        if (found == null) { out.append("not found\n"); writeOut(args, out); return; }
        // xrefs
        ReferenceManager rm = currentProgram.getReferenceManager();
        Set<Function> funcs = new LinkedHashSet<Function>();
        ReferenceIterator it = rm.getReferencesTo(found);
        while (it.hasNext()) {
            Reference r = it.next();
            Address from = r.getFromAddress();
            Function f = currentProgram.getFunctionManager().getFunctionContaining(from);
            out.append("xref from ").append(from.toString())
             .append(" in ").append(f == null ? "<none>" : f.getName()).append('\n');
            if (f != null) funcs.add(f);
        }
        // Also try refs to a data window around the string start (name table entries point at it).
        DecompInterface dec = new DecompInterface();
        dec.openProgram(currentProgram);
        for (Function f : funcs) {
            DecompileResults res = dec.decompileFunction(f, 120, new ConsoleTaskMonitor());
            out.append("\n===== ").append(f.getName()).append(" @ ").append(f.getEntryPoint().toString())
             .append(" =====\n");
            if (res.decompileCompleted()) {
                out.append(res.getDecompiledFunction().getC());
            } else {
                out.append("decompile failed: ").append(res.getErrorMessage());
            }
            out.append('\n');
        }
        dec.dispose();
        writeOut(args, out);
    }
    private void writeOut(String[] args, StringBuilder out) throws IOException {
        if (args.length > 1) {
            PrintWriter w = new PrintWriter(new FileWriter(args[1]));
            w.print(out.toString());
            w.close();
            println("wrote " + args[1]);
        } else {
            println(out.toString());
        }
    }
}

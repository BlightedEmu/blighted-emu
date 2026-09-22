// args[0] = hex function entry address -> list xrefs, decompile each caller
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.util.task.ConsoleTaskMonitor;
import java.io.*;
import java.util.*;

public class XrefDecompile extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Address entry = currentProgram.getAddressFactory().getAddress(args[0]);
        StringBuilder out = new StringBuilder();
        Function target = currentProgram.getFunctionManager().getFunctionAt(entry);
        out.append("target: ").append(target == null ? "?" : target.getName()).append(" @ ").append(entry.toString()).append('\n');
        ReferenceManager rm = currentProgram.getReferenceManager();
        Set<Function> callers = new LinkedHashSet<Function>();
        ReferenceIterator it = rm.getReferencesTo(entry);
        while (it.hasNext()) {
            Reference r = it.next();
            Function f = currentProgram.getFunctionManager().getFunctionContaining(r.getFromAddress());
            out.append("xref from ").append(r.getFromAddress().toString())
             .append(" in ").append(f == null ? "<none>" : f.getName()).append(" @ ")
             .append(f == null ? "?" : f.getEntryPoint().toString()).append('\n');
            if (f != null) callers.add(f);
        }
        DecompInterface dec = new DecompInterface();
        dec.openProgram(currentProgram);
        for (Function f : callers) {
            DecompileResults res = dec.decompileFunction(f, 120, new ConsoleTaskMonitor());
            out.append("\n===== ").append(f.getName()).append(" @ ").append(f.getEntryPoint().toString()).append(" =====\n");
            out.append(res.decompileCompleted() ? res.getDecompiledFunction().getC() : "FAILED " + res.getErrorMessage());
            out.append('\n');
        }
        dec.dispose();
        if (args.length > 1) {
            PrintWriter w = new PrintWriter(new FileWriter(args[1]));
            w.print(out.toString()); w.close();
            println("wrote " + args[1]);
        } else println(out.toString());
    }
}

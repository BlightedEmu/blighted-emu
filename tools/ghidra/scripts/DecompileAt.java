// args[0] = hex address -> decompile containing function
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.util.task.ConsoleTaskMonitor;
import java.io.*;

public class DecompileAt extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Address a = currentProgram.getAddressFactory().getAddress(args[0]);
        Function f = currentProgram.getFunctionManager().getFunctionContaining(a);
        StringBuilder out = new StringBuilder();
        if (f == null) { out.append("no function at ").append(args[0]).append('\n'); }
        else {
            DecompInterface dec = new DecompInterface();
            dec.openProgram(currentProgram);
            DecompileResults res = dec.decompileFunction(f, 120, new ConsoleTaskMonitor());
            out.append(f.getName()).append(" @ ").append(f.getEntryPoint().toString()).append("\n");
            out.append(res.decompileCompleted() ? res.getDecompiledFunction().getC() : "FAILED " + res.getErrorMessage());
            dec.dispose();
        }
        if (args.length > 1) {
            PrintWriter w = new PrintWriter(new FileWriter(args[1]));
            w.print(out.toString()); w.close();
            println("wrote " + args[1]);
        } else println(out.toString());
    }
}

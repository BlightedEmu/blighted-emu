import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.mem.*;

public class FindStr extends GhidraScript {
    public void run() throws Exception {
        String needle = getScriptArgs()[0];
        Memory mem = currentProgram.getMemory();
        Address found = mem.findBytes(currentProgram.getMinAddress(), needle.getBytes(), null, true, monitor);
        while (found != null) {
            printf("found %s at %s\n", needle, found);
            for (ghidra.program.model.symbol.Reference r : getReferencesTo(found)) {
                printf("  xref from %s\n", r.getFromAddress());
            }
            found = mem.findBytes(found.add(1), needle.getBytes(), null, true, monitor);
        }
    }
}

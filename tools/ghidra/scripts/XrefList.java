import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.*;
public class XrefList extends GhidraScript {
    public void run() throws Exception {
        String target = (String)getScriptArgs()[0];
        Address addr = toAddr(Long.parseLong(target, 16));
        for (var ref : getReferencesTo(addr)) {
            Function f = getFunctionContaining(ref.getFromAddress());
            println("xref from " + ref.getFromAddress() + " in " + (f == null ? "<none>" : f.getName() + " @ " + f.getEntryPoint()));
        }
    }
}

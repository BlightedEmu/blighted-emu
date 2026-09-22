// args[0]=hex addr, args[1]=count of qwords
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
public class DumpAt extends GhidraScript {
    public void run() throws Exception {
        Address a = toAddr(Long.parseLong(getScriptArgs()[0],16));
        int n = Integer.parseInt(getScriptArgs()[1]);
        var mem = currentProgram.getMemory();
        for (int i=0;i<n;i++) {
            Address p = a.add(i*8);
            long v = mem.getLong(p);
            String s = "";
            try {
                Address t = toAddr(v);
                if (mem.contains(t)) {
                    Function f = getFunctionContaining(t);
                    if (f!=null) s = " -> func "+f.getName()+"@"+f.getEntryPoint();
                    else {
                        Data d = currentProgram.getListing().getDataAt(t);
                        if (d!=null) s=" -> data "+d;
                    }
                }
            } catch (Exception e) {}
            println(p+" : "+Long.toHexString(v)+s);
        }
    }
}

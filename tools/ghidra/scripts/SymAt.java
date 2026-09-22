import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.symbol.*;

public class SymAt extends GhidraScript {
    public void run() throws Exception {
        for (String arg : getScriptArgs()) {
            Address a = currentProgram.getAddressFactory().getAddress(arg);
            Symbol s = getSymbolAt(a);
            printf("%s -> %s\n", arg, s != null ? s.getName() : "none");
            // also scan back for the nearest symbol
            SymbolIterator it = currentProgram.getSymbolTable().getSymbolIterator();
            // use getSymbol before this address
            Symbol prev = null;
            Address scan = a;
            for (int i = 0; i < 64; i++) {
                scan = scan.subtract(8);
                Symbol sym = getSymbolAt(scan);
                if (sym != null && sym.getName().contains("vtable")) { printf("  prev %s -> %s\n", scan, sym.getName()); break; }
            }
        }
    }
}

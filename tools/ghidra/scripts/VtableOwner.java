import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.scalar.*;

public class VtableOwner extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        for (String arg : args) {
            Address f = currentProgram.getAddressFactory().getAddress(arg);
            for (Reference r : getReferencesTo(f)) {
                Address slot = r.getFromAddress();
                printf("%s: xref from %s\n", arg, slot);
                // vtable start = scan back to find the .data.rel.ro vtable object; RTTI ptr is at vtable-8
                // find a pointer-8-boundary: try scanning back up to 0x200 for an RTTI pointer
                Address a = slot;
                for (int i = 0; i < 0x40; i++) {
                    a = a.subtract(8);
                    try {
                        Scalar s = (Scalar) currentProgram.getListing().getDataAt(a).getValue();
                        if (s != null) {
                            Address target = toAddr(s.getUnsignedValue());
                            Data d = currentProgram.getListing().getDataAt(target);
                            if (d != null) {
                                Symbol sym = getSymbolAt(target);
                                if (sym != null && sym.getName().contains("typeinfo")) {
                                    printf("  vtable RTTI at %s -> %s\n", a, sym.getName());
                                    break;
                                }
                            }
                        }
                    } catch (Exception e) {}
                }
            }
        }
    }
}

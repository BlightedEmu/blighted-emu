// Read-only probe. Prints the program name, the image base, and the function count.
// args[0] = output directory. Writes meta.json there.

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;

public class SweepProbe extends GhidraScript {
    private static final String META_FILE_NAME = "meta.json";

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: SweepProbe <outdir>");
            return;
        }
        File outDir = new File(args[0]);
        if (!outDir.isDirectory() && !outDir.mkdirs()) {
            throw new IOException("cannot create " + outDir.getAbsolutePath());
        }

        String programName = currentProgram.getName();
        Address imageBase = currentProgram.getImageBase();
        String imageBaseHex = "0x" + Long.toHexString(imageBase.getOffset());

        int functionCount = 0;
        FunctionIterator functions = currentProgram.getFunctionManager().getFunctions(true);
        while (functions.hasNext()) {
            Function function = functions.next();
            if (function == null) {
                continue;
            }
            functionCount++;
            if (functionCount % 50000 == 0) {
                println("counted " + functionCount);
            }
        }

        String probedAt = DateTimeFormatter.ISO_INSTANT.format(Instant.now().atOffset(ZoneOffset.UTC));
        StringBuilder body = new StringBuilder();
        body.append("{\n");
        body.append("  \"program_name\": \"").append(escape(programName)).append("\",\n");
        body.append("  \"image_base\": \"").append(imageBaseHex).append("\",\n");
        body.append("  \"function_count\": ").append(functionCount).append(",\n");
        body.append("  \"probed_at\": \"").append(probedAt).append("\"\n");
        body.append("}\n");

        File meta = new File(outDir, META_FILE_NAME);
        File tmp = new File(outDir, META_FILE_NAME + ".tmp");
        Files.writeString(tmp.toPath(), body.toString(), StandardCharsets.UTF_8);
        if (!tmp.renameTo(meta)) {
            Files.move(tmp.toPath(), meta.toPath(), java.nio.file.StandardCopyOption.REPLACE_EXISTING);
        }

        println("program_name=" + programName);
        println("image_base=" + imageBaseHex);
        println("function_count=" + functionCount);
        println("wrote " + meta.getAbsolutePath());
    }

    private static String escape(String value) {
        return value.replace("\\", "\\\\").replace("\"", "\\\"");
    }
}

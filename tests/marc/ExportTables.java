// Export standardized code-point mapping facts through the reference API; no decoder implementation.
import java.io.*;
import org.marc4j.converter.impl.CodeTableGenerated;

public class ExportTables {
    public static void main(String[] args) throws Exception {
        CodeTableGenerated table = new CodeTableGenerated();
        int[] sets = {0x42, 0x45, 0x67, 0x62, 0x70, 0x32, 0x4e, 0x51, 0x33, 0x34, 0x53};
        try (DataOutputStream out = new DataOutputStream(new FileOutputStream(args[0]))) {
            for (int set : sets) for (int code = 0; code < 256; code++) {
                char value = table.getChar(code, set);
                if (value != 0) { out.writeInt((set << 24) | code); out.writeChar(value); out.writeBoolean(table.isCombining(code, set, set)); }
            }
            for (int a = 0x20; a < 0x7f; a++) for (int b = 0x20; b < 0x7f; b++) for (int c = 0x20; c < 0x7f; c++) {
                int code = (a << 16) | (b << 8) | c;
                char value = table.getChar(code, 0x31);
                if (value != 0) { out.writeInt((0x31 << 24) | code); out.writeChar(value); out.writeBoolean(false); }
            }
        }
    }
}

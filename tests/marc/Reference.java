// The same public API configuration used by OpenRefine's MarcImporter; tooling only.
import java.io.*;
import org.marc4j.*;

public class Reference {
    public static void main(String[] args) throws Exception {
        try (InputStream input = new FileInputStream(args[0]); OutputStream output = new FileOutputStream(args[1])) {
            MarcPermissiveStreamReader reader = new MarcPermissiveStreamReader(input, true, true);
            MarcXmlWriter writer = new MarcXmlWriter(output, true);
            while (reader.hasNext()) writer.write(reader.next());
            writer.close();
        }
    }
}

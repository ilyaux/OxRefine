// Reference: arithcode's compressed length and vicino's PPM distance, for each line of the input file
// (UTF-8): prints "<length of line compressed>\t<distance to the next line>" per line.
import com.colloquial.arithcode.ppm.ArithCodeOutputStream;
import com.colloquial.arithcode.ppm.PPMModel;
import edu.mit.simile.vicino.distances.PPMDistance;
import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.List;

public class PpmRef {
  static int len(String s) throws Exception {
    ByteArrayOutputStream out = new ByteArrayOutputStream();
    ArithCodeOutputStream a = new ArithCodeOutputStream(out, new PPMModel(8));
    a.write(s.getBytes(StandardCharsets.UTF_8));
    a.close();
    out.close();
    return out.toByteArray().length;
  }

  public static void main(String[] args) throws Exception {
    List<String> lines = Files.readAllLines(Paths.get(args[0]), StandardCharsets.UTF_8);
    PPMDistance d = new PPMDistance();
    for (int i = 0; i < lines.size(); i++) {
      String a = lines.get(i), b = lines.get((i + 1) % lines.size());
      System.out.println(len(a) + "\t" + d.d(a, b));
    }
  }
}

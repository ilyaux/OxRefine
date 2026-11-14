// Reference API outputs only; compile with javac --release 11, run with OpenRefine's bundled JRE.
import java.io.*;
import java.lang.reflect.Field;
import java.nio.file.*;
import java.util.*;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.optimaize.langdetect.*;
import com.optimaize.langdetect.ngram.NgramExtractors;
import com.optimaize.langdetect.profiles.LanguageProfileReader;
import com.optimaize.langdetect.cybozu.util.CharNormalizer;

public class ExportModel {
    public static void main(String[] args) throws Exception {
        LanguageDetector detector = LanguageDetectorBuilder.create(NgramExtractors.standard())
            .withProfiles(new LanguageProfileReader().readAllBuiltIn()).build();
        Field field = detector.getClass().getDeclaredField("ngramFrequencyData");
        field.setAccessible(true);
        NgramFrequencyData data = (NgramFrequencyData) field.get(detector);
        Map<String, Object> result = new LinkedHashMap<>();
        List<String> order = new ArrayList<>();
        data.getLanguageList().forEach(locale -> order.add(locale.toString()));
        result.put("languageOrder", order);
        result.put("javaVersion", System.getProperty("java.version"));
        Map<String, Integer> scripts = new LinkedHashMap<>();
        for (Character.UnicodeScript script : Character.UnicodeScript.values()) scripts.put(script.name(), script.ordinal());
        result.put("scripts", scripts);
        try (DataOutputStream out = new DataOutputStream(Files.newOutputStream(Paths.get(args[0])))) {
            for (int c = 0; c < 65536; c++) {
                out.writeChar(CharNormalizer.normalize((char)c));
                out.writeByte(Character.UnicodeScript.of(c).ordinal());
            }
        }
        Random gaussian = new Random(41);
        List<Double> values = new ArrayList<>();
        for (int i = 0; i < 20; i++) values.add(gaussian.nextGaussian());
        result.put("gaussians", values);
        List<String> bits = new ArrayList<>();
        for (double value : values) bits.add(String.format("%016x", Double.doubleToLongBits(value)));
        result.put("gaussianBits", bits);
        Random random = new Random(41);
        List<Integer> integers = new ArrayList<>();
        int[] bounds = {1, 2, 3, 7, 100, 1000, 65536, 1000000007};
        for (int bound : bounds) integers.add(random.nextInt(bound));
        result.put("randomBounds", bounds);
        result.put("randomIntegers", integers);
        new ObjectMapper().writeValue(Paths.get(args[1]).toFile(), result);
    }
}

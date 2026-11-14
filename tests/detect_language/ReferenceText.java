// Trace the real library's preprocessing for edge cases; no detector implementation here.
import java.nio.file.*;
import java.util.*;
import com.fasterxml.jackson.databind.*;
import com.optimaize.langdetect.text.CommonTextObjectFactories;

public class ReferenceText {
    public static void main(String[] args) throws Exception {
        ObjectMapper mapper = new ObjectMapper();
        JsonNode cases = mapper.readTree(Paths.get(args[0]).toFile());
        Map<String, Object> out = new LinkedHashMap<>();
        for (JsonNode item : cases) {
            if (!item.path("trace").asBoolean()) continue;
            CharSequence normalized = CommonTextObjectFactories.forDetectingOnLargeText().forText(item.get("text").asText());
            int[] units = new int[normalized.length()];
            for (int i = 0; i < units.length; i++) units[i] = normalized.charAt(i);
            out.put(item.get("name").asText(), units);
        }
        mapper.writeValue(Paths.get(args[1]).toFile(), out);
    }
}

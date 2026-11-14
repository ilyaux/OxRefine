package org.openrefine.extensions.commons.importer;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.google.refine.model.Cell;
import com.google.refine.util.ParsingUtilities;

/** Runs the unmodified CommonsExtension mapping code without fetching Commons. */
public final class CommonsMappingReference {
    private static ObjectNode runCase(String name, List<FileRecord> files,
            boolean categoriesColumn, boolean mIdsColumn) throws Exception {
        ObjectNode test = ParsingUtilities.mapper.createObjectNode();
        test.put("name", name);
        ObjectNode input = test.putObject("input");
        input.put("categoriesColumn", categoriesColumn);
        input.put("mIdsColumn", mIdsColumn);
        ArrayNode fileInputs = input.putArray("files");
        for (FileRecord file : files) {
            ObjectNode fileInput = fileInputs.addObject();
            fileInput.put("fileName", file.fileName);
            fileInput.put("pageId", file.pageId);
            fileInput.set("relatedCategories", ParsingUtilities.mapper.valueToTree(file.relatedCategories));
            fileInput.put("error", file.error);
        }
        ArrayNode rows = test.putObject("expected").putArray("rows");
        FileRecordToRows reader = new FileRecordToRows(files.iterator(), categoriesColumn, mIdsColumn);
        List<Object> values;
        while ((values = reader.getNextRowOfCells()) != null) {
            ArrayNode row = rows.addArray();
            for (Object value : values) {
                if (value == null) {
                    row.addNull();
                } else {
                    ObjectNode cell = row.addObject();
                    cell.set("v", ParsingUtilities.mapper.valueToTree(value instanceof Cell ? ((Cell) value).value : value));
                    if (value instanceof Cell && ((Cell) value).recon != null) {
                        ObjectNode recon = (ObjectNode) ParsingUtilities.mapper.readTree(
                                ParsingUtilities.saveWriter.writeValueAsString(((Cell) value).recon));
                        recon.remove("id"); // Generated identity changes between identical runs.
                        cell.set("recon", recon);
                    }
                }
            }
        }
        return test;
    }

    public static void main(String[] args) throws Exception {
        ObjectNode output = ParsingUtilities.mapper.createObjectNode();
        ArrayNode cases = output.putArray("cases");
        FileRecord first = new FileRecord("File:Example alpha.jpg", "123", null, null);
        FileRecord second = new FileRecord("File:Unicode \u03b1 & \"quote\".png", "456", null, null);
        cases.add(runCase("file-only", Arrays.asList(first, second), false, false));
        cases.add(runCase("empty", Collections.emptyList(), false, false));
        cases.add(runCase("duplicate-files", Arrays.asList(first, first), false, false));
        cases.add(runCase("mids", Arrays.asList(first, second), false, true));
        List<FileRecord> withCategories = Arrays.asList(
                new FileRecord(first.fileName, first.pageId,
                        Arrays.asList("Category:First", "Category:Second"), null), second);
        cases.add(runCase("categories", withCategories, true, false));
        cases.add(runCase("categories-and-mids", withCategories, true, true));
        cases.add(runCase("category-fetch-error", Collections.singletonList(
                new FileRecord(first.fileName, first.pageId, null, "Could not fetch related categories: synthetic failure")), true, false));

        JsonNode firstRow = cases.get(0).path("expected").path("rows").get(0);
        if (!firstRow.get(0).path("v").asText().equals(first.fileName)
                || !firstRow.get(0).path("recon").path("m").path("id").asText().equals("M123")
                || cases.get(4).path("expected").path("rows").size() != 3) {
            throw new IllegalStateException("Reference mapping self-check failed: " + firstRow
                    + "; category rows=" + cases.get(4).path("expected").path("rows").size());
        }
        Files.writeString(Path.of(args[0]), ParsingUtilities.mapper.writerWithDefaultPrettyPrinter()
                .writeValueAsString(output) + "\n", StandardCharsets.UTF_8);
    }
}

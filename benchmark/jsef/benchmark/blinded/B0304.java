package blinded;

import javax.xml.parsers.DocumentBuilder;
import javax.xml.parsers.DocumentBuilderFactory;
import java.io.ByteArrayInputStream;

/*
 * JSEF-Benchmark L3 — SVG XXE 修复（CWE-611）
 *
 * 修复：在用于解析的同一 DocumentBuilderFactory 上禁用 DOCTYPE 声明。
 *
 * CWE-611 (Improper Restriction of XML External Entity Reference)。
 */
public class SvgXxeBy {

    static final String DISALLOW = "http://apache.org/xml/features/disallow-doctype-decl";

    




    public void parse(byte[] svgBytes) throws Exception {
        DocumentBuilderFactory dbf = DocumentBuilderFactory.newInstance();
        dbf.setFeature(DISALLOW, true); // 禁用 DOCTYPE → 阻断 XXE
        DocumentBuilder db = dbf.newDocumentBuilder();
        /*ANCHOR_1*/
        db.parse(new ByteArrayInputStream(svgBytes)); // 外部实体被禁
    }

    public static void main(String[] args) throws Exception {
        new SvgXxeBy().parse("<svg/>".getBytes());
    }
}

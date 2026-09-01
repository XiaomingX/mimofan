package blinded;

import javax.xml.parsers.DocumentBuilder;
import javax.xml.parsers.DocumentBuilderFactory;
import org.w3c.dom.Document;









public class L0XxeDirectBy {

    




    public void run(DocumentBuilderFactory dbf, String xml) throws Exception {
        dbf.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);
        dbf.setFeature("http://xml.org/sax/features/external-general-entities", false);
        dbf.setFeature("http://xml.org/sax/features/external-parameter-entities", false);
        dbf.setXIncludeAware(false);
        DocumentBuilder builder = dbf.newDocumentBuilder();
        /*ANCHOR_1*/
        Document doc = builder.parse(new java.io.ByteArrayInputStream(xml.getBytes()));
    }

    public static void main(String[] args) {
        System.out.println("demo: by parse localhost-demo xml (no external entities)");
    }
}

package blinded;

import javax.xml.parsers.DocumentBuilder;
import javax.xml.parsers.DocumentBuilderFactory;
import org.w3c.dom.Document;










public class L0XxeDirect {

    




    public void run(DocumentBuilderFactory dbf, String xml) throws Exception {
        DocumentBuilder builder = dbf.newDocumentBuilder();
        /*ANCHOR_1*/
        Document doc = builder.parse(new java.io.ByteArrayInputStream(xml.getBytes()));
    }

    public static void main(String[] args) {
        System.out.println("demo: parse localhost-demo xml");
    }
}

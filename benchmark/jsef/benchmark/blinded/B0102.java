package blinded;

import java.net.URLDecoder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;

/*
 * JSEF-Benchmark L3 — 双重 URL 解码修复
 *
 * 修复：先一次性把路径 canonicalize 到最终形态（此处直接以原始串做一次权威解码 +
 * normalize），再校验规范化结果不含 ".." 且位于 BASE_DIR 内。全链路只解码一次，
 * 杜绝校验/使用不一致的窗口。
 *
 * CWE-22。BX 侧按实现判安全。
 */
public class DoubleDecodeTraversalBy {

    private static final String BASE_DIR = "/var/www/uploads";

    




    public void write(String userPath) throws Exception {
        // 仅做一次权威解码，得到最终形态
        String canonical = URLDecoder.decode(userPath, StandardCharsets.UTF_8);
        Path p = Paths.get(BASE_DIR).resolve(canonical).normalize();
        if (canonical.contains("..") || !p.startsWith(Paths.get(BASE_DIR).normalize())) {
            throw new SecurityException("path traversal blocked");
        }
        byte[] data = "demo".getBytes();
        /*ANCHOR_1*/
        Files.write(p, data);
    }

    public static void main(String[] args) throws Exception {
        new DoubleDecodeTraversalBy().write("%252e%252e%252f%252e%252e%252fetc%252fpasswd");
    }
}

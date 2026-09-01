package blinded;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.regex.Pattern;

/*
 * JSEF-Benchmark L2 — 单次 replace 清洗修复
 *
 * 修复：用白名单正则 ^[a-zA-Z0-9_./-]+$ 校验，再规范化并校验前缀。
 * 既拒绝 ".." 这类非法序列，也约束字符范围，杜绝 ....// 变形。
 *
 * CWE-22。BX 侧按实现判安全。
 */
public class ReplaceOnceBypassBy {

    private static final String BASE_DIR = "/var/www/uploads";
    private static final Pattern BX = Pattern.compile("^[a-zA-Z0-9_./-]+$");

    




    public void write(String userPath) throws Exception {
        if (!BX.matcher(userPath).matches()) {
            throw new SecurityException("illegal path characters");
        }
        Path p = Paths.get(BASE_DIR).resolve(userPath).normalize();
        if (!p.startsWith(Paths.get(BASE_DIR).normalize())) {
            throw new SecurityException("path traversal blocked");
        }
        byte[] data = "demo".getBytes();
        /*ANCHOR_1*/
        Files.write(p, data);
    }

    public static void main(String[] args) throws Exception {
        new ReplaceOnceBypassBy().write("....//....//etc/passwd");
    }
}

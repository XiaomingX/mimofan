/*
 * JSEF Benchmark 样本 — IDOR 安全对照 (CWE-639, L2/L3)
 * 每次访问前校验资源归属当前用户。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;

public class IdorBy {

    interface Repo { String findById(long id); boolean owns(long id, long user); }

    static String getProfile(Repo repo, long userId, long currentUser) {
        if (!repo.owns(userId, currentUser)) throw new SecurityException("forbidden");
        /*ANCHOR_1*/
        return repo.findById(userId);
    }

    static String readFile(String filePath, String owner) throws Exception {
        if (!filePath.startsWith("/data/" + owner + "/")) throw new SecurityException("forbidden");
        /*ANCHOR_2*/
        return new String(Files.readAllBytes(Paths.get(filePath)));
    }

    static List<String> export(Repo repo, long from, long to, long currentUser) {
        List<String> out = new ArrayList<>();
        for (long id = from; id <= to; id++) {
            if (!repo.owns(id, currentUser)) continue; // 逐条校验
            /*ANCHOR_3*/
            out.add(repo.findById(id));
        }
        return out;
    }
}

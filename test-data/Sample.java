import java.util.List;

/**
 * Test fixture for peek's Java classfile viewer. Exercises varied field
 * and method shapes — access modifiers, primitive / array / object
 * types, void and value returns, a generic interface — so the Info,
 * Fields and Methods views all have content to render. Compile with
 * `javac -d test-data test-data/Sample.java` to refresh the committed
 * `Sample.class` fixture.
 */
public class Sample implements Comparable<Sample> {

    public static final int VERSION = 3;
    private String name;
    protected double[] measurements;
    private final List<String> tags;

    public Sample(String name) {
        this.name = name;
        this.tags = List.of();
    }

    public String getName() {
        return name;
    }

    public void setName(String value) {
        this.name = value;
    }

    protected double average(double[] values) {
        double sum = 0;
        for (double v : values) {
            sum += v;
        }
        return values.length == 0 ? 0 : sum / values.length;
    }

    private static boolean isBlank(String s) {
        return s == null || s.isEmpty();
    }

    @Override
    public int compareTo(Sample other) {
        return name.compareTo(other.name);
    }
}

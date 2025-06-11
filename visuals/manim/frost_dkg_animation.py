from manim import *
import numpy as np

# Styles and color constants
NODE_COLOR_INACTIVE = GREY_A
NODE_COLOR_ACTIVE = GREEN_E
ARROW_COLOR = BLACK
BROADCAST_COLOR = BLUE_D
SHARE_COLOR = GREEN_B
STAGE3_ARROW_COLOR = BLUE_D
TICK_COLOR = GREEN_E
STAGE_LABEL_PAD = 0.6


class NodeBox(VGroup):
    """A rounded rectangle with a label that can be toggled active / inactive."""

    def __init__(self, label: str, **kwargs):
        super().__init__(**kwargs)
        self.label = label
        # rectangle
        self.rect = RoundedRectangle(
            corner_radius=0.2, width=2.0, height=1.0, color=NODE_COLOR_INACTIVE
        )
        self.rect.set_fill(NODE_COLOR_INACTIVE, opacity=0.4)
        # text label
        self.text = Text(label, font_size=28)
        self.text.move_to(self.rect.get_center())

        self.add(self.rect, self.text)

    # visual helpers
    def activate(self):
        return AnimationGroup(
            self.rect.animate.set_fill(NODE_COLOR_ACTIVE, opacity=0.8).set_color(
                NODE_COLOR_ACTIVE
            ),
            self.text.animate.set_color(WHITE),
        )

    def deactivate(self):
        return AnimationGroup(
            self.rect.animate.set_fill(NODE_COLOR_INACTIVE, opacity=0.4).set_color(
                NODE_COLOR_INACTIVE
            ),
            self.text.animate.set_color(WHITE),
        )

    def edge_point(self, target: np.ndarray, margin: float = 0.1) -> np.ndarray:
        """Return a point on the rectangle's edge in the direction of *target*."""
        direction = target - self.get_center()
        direction_norm = np.linalg.norm(direction)
        if direction_norm == 0:
            return self.get_center()
        unit = direction / direction_norm
        # Use max(width, height) / 2 to step outside the rectangle.
        border = max(self.rect.width, self.rect.height) / 2
        return self.get_center() + unit * (border + margin)


class FrostDKGScene(Scene):
    """Main animation – visual walk-through of 3-party FROST DKG (no maths, only visuals)."""

    def construct(self):
        # ----- Layout helpers ------------------------------------------------
        # triangle coordinates for the three nodes (lowered to avoid text overlap)
        p1_pos = LEFT * 3 + UP * 0.5
        p2_pos = RIGHT * 3 + UP * 0.5
        p3_pos = DOWN * 2

        # create node boxes
        p1 = NodeBox("P1").move_to(p1_pos)
        p2 = NodeBox("P2").move_to(p2_pos)
        p3 = NodeBox("P3").move_to(p3_pos)

        nodes = [p1, p2, p3]

        # initial node fade-in
        self.play(*[FadeIn(node, shift=DOWN, scale=0.8) for node in nodes])
        # ensure group is centred
        nodes_group = VGroup(*nodes)
        self.play(nodes_group.animate.move_to(ORIGIN))
        self.wait(0.5)

        # ------------------------------------------------- helper for brief text
        def brief(txt: str):
            info = Text(txt, font_size=28)
            info.to_edge(DOWN, buff=0.6)
            self.play(Write(info))
            self.wait(2)
            self.play(FadeOut(info))

        # --------------------------------------------------------------------
        # STAGE 1 – Commitments broadcast
        stage1_label = Text("Stage 1 – Commitments", font_size=38)
        stage1_label.to_edge(UP, buff=STAGE_LABEL_PAD)

        self.play(FadeIn(stage1_label, shift=DOWN))
        self.wait(0.3)

        # quick explainer text
        brief("Each node chooses a secret curve & broadcasts commitments")

        # helper function for broadcasting arrows
        def arrow_between(a: NodeBox, b: NodeBox, color: str, width: int = 4):
            return Arrow(
                start=a.edge_point(b.get_center()),
                end=b.edge_point(a.get_center()),
                buff=0,
                color=color,
                stroke_width=width,
            )

        def broadcast_with_labels(from_node: NodeBox, others: list[NodeBox]):
            arrows = VGroup()
            labels = VGroup()
            for idx, target in enumerate(others, start=1):
                arrow = arrow_between(from_node, target, BROADCAST_COLOR, 4)
                arrows.add(arrow)
                lbl = Tex(r"$g^{c_{%d}}$" % idx, font_size=26, color=BROADCAST_COLOR)
                lbl.move_to(arrow.get_center() + UP * 0.3)
                labels.add(lbl)
            return arrows, labels

        def broadcast(from_node: NodeBox, others: list[VGroup]):
            return VGroup(
                *[
                    arrow_between(from_node, target, BROADCAST_COLOR, 4)
                    for target in others
                ]
            )

        # animate each node broadcasting commitments
        for active, others in ((p1, [p2, p3]), (p2, [p1, p3]), (p3, [p1, p2])):
            # highlight active node
            self.play(
                active.activate(), *[n.deactivate() for n in nodes if n != active]
            )

            # show simple polynomial representation above active node
            poly = Tex(r"$f(x)=a_0+a_1x$", font_size=26, color=WHITE)
            poly.next_to(active, UP, buff=0.4)
            self.play(FadeIn(poly))

            arrows, labels = broadcast_with_labels(active, others)
            self.play(Create(arrows, run_time=0.8))
            self.play(FadeIn(labels))
            self.wait(0.3)
            self.play(FadeOut(arrows), FadeOut(labels), FadeOut(poly))

        self.wait(1)
        self.play(FadeOut(stage1_label))

        # --------------------------------------------------------------------
        # STAGE 2 – Share distribution & verification
        stage2_label = Text("Stage 2 – Share distribution", font_size=38)
        stage2_label.to_edge(UP, buff=STAGE_LABEL_PAD)

        self.play(FadeIn(stage2_label, shift=DOWN))
        self.wait(0.3)

        brief("Nodes exchange secret shares and verify them")

        def share_with_label(from_node: NodeBox, to_node: NodeBox):
            arrow = arrow_between(from_node, to_node, SHARE_COLOR, 6)
            lbl = Tex(r"$s_{ij}$", font_size=24, color=SHARE_COLOR)
            lbl.move_to(arrow.get_center() + UP * 0.25)
            return arrow, lbl

        def tick_at(target: Mobject):
            tick = Text("✓", font_size=36, color=TICK_COLOR)
            tick.next_to(target, direction=RIGHT, buff=0.1)
            return tick

        for active, peers in ((p1, [p2, p3]), (p2, [p1, p3]), (p3, [p1, p2])):
            self.play(
                active.activate(), *[n.deactivate() for n in nodes if n != active]
            )

            arrows = VGroup()
            labels = VGroup()
            for peer in peers:
                arrow, lbl = share_with_label(active, peer)
                arrows.add(arrow)
                labels.add(lbl)

            self.play(GrowArrow(arrows[0]), GrowArrow(arrows[1]), run_time=1.0)
            self.play(FadeIn(labels))
            ticks = VGroup(*[tick_at(peer) for peer in peers])
            self.play(FadeIn(ticks, scale=0.3))
            self.wait(0.5)
            self.play(FadeOut(arrows), FadeOut(labels), FadeOut(ticks))

        self.wait(1)
        self.play(FadeOut(stage2_label))

        # --------------------------------------------------------------------
        # STAGE 3 – Combine to form group keys
        stage3_label = Text("Stage 3 – Combine keys", font_size=38)
        stage3_label.to_edge(UP, buff=STAGE_LABEL_PAD)
        self.play(FadeIn(stage3_label, shift=DOWN))
        self.wait(0.5)

        brief("Commitments ➜ group public key & Shares ➜ group private key")

        top_y = 1.3
        self.play(
            p1.animate.move_to(LEFT * 4 + UP * top_y),
            p2.animate.move_to(RIGHT * 4 + UP * top_y),
            p3.animate.move_to(UP * top_y),
        )

        # -----------------  Centered key labels + equations  -----------------
        combine_text = Text("Group public key", font_size=24)
        pub_eq = Tex(
            r"$P_{\text{group}} = g^{a_0^{(1)} + a_0^{(2)} + a_0^{(3)}}$",
            font_size=24,
        )
        pub_eq.next_to(combine_text, RIGHT, buff=0.2)
        pub_group = VGroup(combine_text, pub_eq)
        pub_group.next_to(p3, DOWN, buff=3.0)

        secret_text = Text("Group private key", font_size=24)
        priv_eq = Tex(r"$s_{\text{group}} = s_{0}+s_{1}+s_{2}$", font_size=24)
        priv_eq.next_to(secret_text, RIGHT, buff=0.2)
        secret_group = VGroup(secret_text, priv_eq)
        secret_group.next_to(pub_group, DOWN, buff=0.4)

        # downward arrows into the text with smaller arrow heads
        arrow1 = Arrow(
            start=p1.get_bottom(),
            end=pub_group.get_top(),
            buff=0,
            color=STAGE3_ARROW_COLOR,
            stroke_width=6,
            max_tip_length_to_length_ratio=0.1,  # smaller arrow head
        )
        arrow2 = Arrow(
            start=p2.get_bottom(),
            end=pub_group.get_top(),
            buff=0,
            color=STAGE3_ARROW_COLOR,
            stroke_width=6,
            max_tip_length_to_length_ratio=0.1,  # smaller arrow head
        )
        arrow3 = Arrow(
            start=p3.get_bottom(),
            end=pub_group.get_top(),
            buff=0,
            color=STAGE3_ARROW_COLOR,
            stroke_width=6,
            max_tip_length_to_length_ratio=0.1,  # smaller arrow head
        )

        # label the secret pieces with subscript
        s0 = Tex(r"$s_{0}$", font_size=24, color=STAGE3_ARROW_COLOR).next_to(
            arrow1, LEFT, buff=0.01
        )
        s1 = Tex(r"$s_{1}$", font_size=24, color=STAGE3_ARROW_COLOR).next_to(
            arrow2, RIGHT, buff=0.01
        )
        s2 = Tex(r"$s_{2}$", font_size=24, color=STAGE3_ARROW_COLOR).next_to(
            arrow3, RIGHT, buff=0.01
        )

        down_arrows = VGroup(arrow1, arrow2, arrow3)
        share_labels = VGroup(s0, s1, s2)

        self.play(Create(down_arrows, lag_ratio=0.2))
        self.play(FadeIn(share_labels))
        self.play(FadeIn(pub_group))
        self.play(FadeIn(secret_group))
        self.wait(2)
        brief("Threshold signing possible – any 2 of 3 shares can sign")

use anchor_lang::prelude::*;
use anchor_lang::system_program;

declare_id!("AvTfTNzZfqg666MTy6N4MaeMwdZxa8rBGgdsgkdGoXPK");

#[program]
pub mod lp_program {
    use super::*;

    pub fn register_user(ctx: Context<RegisterUser>, name: String, role: UserRole) -> Result<()> {
        let user = &mut ctx.accounts.user_account;
        user.wallet = ctx.accounts.signer.key();
        user.name = name;
        user.role = role;

        msg!("User registered: {} as {:?}", user.name, user.role);
        Ok(())
    }

    // Note: start_date and end_date are i64 unix timestamps (seconds)
    #[allow(clippy::too_many_arguments)]
    pub fn initialize_job_post(
        ctx: Context<InitializeJobPost>,
        title: String,
        description: String,
        amount: u64,
        start_date: i64,
        end_date: i64,
    ) -> Result<()> {
        // Only clients can post jobs
        require!(
            ctx.accounts.user_account.role == UserRole::Client,
            ErrorCode::Unauthorized
        );

        // Validation: start_date must be <= end_date, and start_date must not be in the past
        require!(start_date <= end_date, ErrorCode::InvalidDates);

        let clock = Clock::get()?;
        require!(start_date >= clock.unix_timestamp, ErrorCode::InvalidDates);

        let job_post = &mut ctx.accounts.job_post;
        job_post.client = ctx.accounts.user_account.wallet;
        job_post.title = title;
        job_post.description = description;
        job_post.amount = amount;
        job_post.is_filled = false;
        job_post.escrow_bump = ctx.bumps.escrow;
        job_post.start_date = start_date;
        job_post.end_date = end_date;

        // Transfer funds to escrow
        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.signer.to_account_info(),
                to: ctx.accounts.escrow.to_account_info(),
            },
        );
        system_program::transfer(cpi_context, amount)?;

        msg!(
            "Job post created with amount: {} start: {} end: {}",
            amount,
            job_post.start_date,
            job_post.end_date
        );
        Ok(())
    }

    // Allow freelancer to include expected_end_date when applying
    pub fn apply_to_job(
        ctx: Context<ApplyToJob>,
        resume_link: String,
        expected_end_date: i64,
    ) -> Result<()> {
        // Only freelancers can apply
        require!(
            ctx.accounts.user_account.role == UserRole::Freelancer,
            ErrorCode::Unauthorized
        );

        // Validation: freelancer's expected_end_date must be a valid unix timestamp (non-negative)
        require!(expected_end_date >= 0, ErrorCode::InvalidDates);

        let application = &mut ctx.accounts.application;
        application.applicant = ctx.accounts.user_account.wallet;
        application.job_post = ctx.accounts.job_post.key();
        application.resume_link = resume_link;
        application.approved = false;
        application.completed = false;
        application.submission_link = String::new();
        application.narration = String::new();
        application.client_review = String::new();
        application.expected_end_date = expected_end_date;

        msg!(
            "Application submitted with resume: {} expected_end_date: {}",
            application.resume_link,
            application.expected_end_date
        );
        Ok(())
    }

    pub fn approve_application(ctx: Context<ApproveApplication>) -> Result<()> {
        require!(
            ctx.accounts.job_post.client == ctx.accounts.user_account.wallet,
            ErrorCode::Unauthorized
        );
        require!(
            ctx.accounts.user_account.role == UserRole::Client,
            ErrorCode::Unauthorized
        );
        require!(
            !ctx.accounts.job_post.is_filled,
            ErrorCode::JobAlreadyFilled
        );

        let application = &mut ctx.accounts.application;
        application.approved = true;
        ctx.accounts.job_post.is_filled = true;

        msg!(
            "Application approved for job: {}",
            ctx.accounts.job_post.title
        );
        Ok(())
    }

    pub fn submit_work(
        ctx: Context<SubmitWork>,
        submission_link: String,
        narration: String,
    ) -> Result<()> {
        require!(
            ctx.accounts.user_account.role == UserRole::Freelancer,
            ErrorCode::Unauthorized
        );
        require!(
            ctx.accounts.application.applicant == ctx.accounts.user_account.wallet,
            ErrorCode::Unauthorized
        );
        require!(
            ctx.accounts.application.approved,
            ErrorCode::ApplicationNotApproved
        );

        let application = &mut ctx.accounts.application;
        application.submission_link = submission_link;
        application.narration = narration;
        application.completed = true;

        msg!(
            "Work submitted with link: {} and narration",
            application.submission_link
        );
        Ok(())
    }

    pub fn approve_submission(
        ctx: Context<ApproveSubmission>,
        client_review: String,
    ) -> Result<()> {
        require!(
            ctx.accounts.job_post.client == ctx.accounts.user_account.wallet,
            ErrorCode::Unauthorized
        );
        require!(
            ctx.accounts.user_account.role == UserRole::Client,
            ErrorCode::Unauthorized
        );
        require!(
            ctx.accounts.application.completed,
            ErrorCode::WorkNotCompleted
        );

        // Save client review
        let application = &mut ctx.accounts.application;
        application.client_review = client_review;

        // Transfer funds from escrow to freelancer
        let job_post_key = ctx.accounts.job_post.key();
        let seeds = &[
            b"escrow",
            job_post_key.as_ref(),
            &[ctx.accounts.job_post.escrow_bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.escrow.to_account_info(),
                to: ctx.accounts.freelancer.to_account_info(),
            },
            signer,
        );
        system_program::transfer(cpi_context, ctx.accounts.job_post.amount)?;

        msg!("Submission approved, funds transferred, and review recorded");
        Ok(())
    }
}

#[account]
#[derive(InitSpace)]
pub struct UserAccount {
    pub wallet: Pubkey,
    #[max_len(50)]
    pub name: String,
    pub role: UserRole,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, InitSpace, Debug)]
pub enum UserRole {
    Client,
    Freelancer,
}

#[account]
#[derive(InitSpace)]
pub struct JobPost {
    pub client: Pubkey,
    #[max_len(100)]
    pub title: String,
    pub amount: u64,
    #[max_len(500)]
    pub description: String,
    pub is_filled: bool,
    pub escrow_bump: u8,
    // New: start and end dates (unix timestamps, in seconds)
    pub start_date: i64,
    pub end_date: i64,
}

#[account]
#[derive(InitSpace)]
pub struct Application {
    pub applicant: Pubkey,
    pub job_post: Pubkey,
    #[max_len(200)]
    pub resume_link: String,
    pub approved: bool,
    pub completed: bool,
    #[max_len(200)]
    pub submission_link: String,
    #[max_len(300)]
    pub narration: String, // freelancer’s narration
    #[max_len(300)]
    pub client_review: String, // client’s review
    // New: freelancer's expected end date for the job (unix timestamp, in seconds)
    pub expected_end_date: i64,
}

#[derive(Accounts)]
pub struct RegisterUser<'info> {
    #[account(
        init,
        payer = signer,
        space = 8 + UserAccount::INIT_SPACE,
        seeds = [b"user", signer.key().as_ref()],
        bump
    )]
    pub user_account: Account<'info, UserAccount>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(title: String, start_date: i64, end_date: i64)]
pub struct InitializeJobPost<'info> {
    #[account(
        init,
        payer = signer,
        space = 8 + JobPost::INIT_SPACE,
        seeds = [b"job_post", signer.key().as_ref(), title.as_bytes()],
        bump
    )]
    pub job_post: Account<'info, JobPost>,

    #[account(
        init,
        payer = signer,
        seeds = [b"escrow", job_post.key().as_ref()],
        bump,
        space = 8
    )]
    /// CHECK: Escrow account
    pub escrow: UncheckedAccount<'info>,

    #[account(mut)]
    pub signer: Signer<'info>,
    pub user_account: Account<'info, UserAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ApplyToJob<'info> {
    #[account(
        init,
        payer = signer,
        space = 8 + Application::INIT_SPACE,
        seeds = [b"application", job_post.key().as_ref(), signer.key().as_ref()],
        bump
    )]
    pub application: Account<'info, Application>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub user_account: Account<'info, UserAccount>,
    pub job_post: Account<'info, JobPost>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ApproveApplication<'info> {
    #[account(mut)]
    pub application: Account<'info, Application>,
    #[account(mut)]
    pub job_post: Account<'info, JobPost>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub user_account: Account<'info, UserAccount>,
}

#[derive(Accounts)]
pub struct SubmitWork<'info> {
    #[account(mut)]
    pub application: Account<'info, Application>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub user_account: Account<'info, UserAccount>,
    pub job_post: Account<'info, JobPost>,
}

#[derive(Accounts)]
pub struct ApproveSubmission<'info> {
    #[account(mut)]
    pub application: Account<'info, Application>,
    #[account(mut)]
    pub job_post: Account<'info, JobPost>,
    #[account(
        mut,
        seeds = [b"escrow", job_post.key().as_ref()],
        bump = job_post.escrow_bump
    )]
    /// CHECK: Escrow
    pub escrow: UncheckedAccount<'info>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub user_account: Account<'info, UserAccount>,
    #[account(mut)]
    /// CHECK: Freelancer
    pub freelancer: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum ErrorCode {
    #[msg("You are not authorized to perform this action")]
    Unauthorized,
    #[msg("This job has already been filled")]
    JobAlreadyFilled,
    #[msg("Application has not been approved yet")]
    ApplicationNotApproved,
    #[msg("Work has not been completed yet")]
    WorkNotCompleted,
    #[msg("Invalid dates provided")]
    InvalidDates,
}
